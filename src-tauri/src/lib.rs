mod db;
mod llm;
mod templates;

use std::sync::Mutex;
use std::time::Duration;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{Emitter, Listener, Manager, State, WebviewWindow, WindowEvent};

const CANVAS: &str = "canvas";
const SHELL: &str = "shell";
/// Drawer width in logical pixels.
const DRAWER_WIDTH: f64 = 380.0;
/// Collapsed width. The drawer never disappears — a control you cannot see is a
/// control you cannot find, and this one is how you undo a bad generation.
const DRAWER_RAIL: f64 = 26.0;
/// Gap between app and drawer. Deliberately not zero: a decorated macOS window has
/// rounded corners, so butting a square panel against it leaves a notch. A small
/// gap reads as two deliberate panels instead of one broken surface.
const DRAWER_GAP: f64 = 12.0;

pub struct Db(Mutex<Connection>);

impl Db {
    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T, String>) -> Result<T, String> {
        let conn = self.0.lock().map_err(|_| "database lock poisoned".to_string())?;
        f(&conn)
    }
}

fn canvas(app: &tauri::AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window(CANVAS)
        .ok_or_else(|| "canvas window not found".to_string())
}

fn shell(app: &tauri::AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window(SHELL)
        .ok_or_else(|| "shell window not found".to_string())
}

/// Keep the drawer flush against the canvas's left edge, same height.
///
/// Two windows rather than two webviews is deliberate. Tauri scopes capabilities
/// by window, so this is what keeps `save_module` and the API key out of reach of
/// model-authored code. Multi-webview would allow a single window, but it is still
/// behind the `unstable` flag with open positioning and resize bugs.
///
/// Still required even though the shell declares `"parent": "canvas"`. That makes
/// the drawer a macOS child window — which is what keeps the pair together in
/// Mission Control and raises them as a unit — but a child window follows its
/// parent's *position* only. It does not track the parent's height, and it knows
/// nothing about the collapsed rail width, so both are set here.
fn dock_drawer(app: &tauri::AppHandle) -> Result<(), String> {
    let canvas = canvas(app)?;
    let drawer = shell(app)?;

    let pos = canvas.outer_position().map_err(|e| e.to_string())?;
    let size = canvas.outer_size().map_err(|e| e.to_string())?;
    let scale = canvas.scale_factor().map_err(|e| e.to_string())?;
    let gap = (DRAWER_GAP * scale).round() as i32;

    let logical = if is_collapsed(&drawer, scale) { DRAWER_RAIL } else { DRAWER_WIDTH };
    let width = (logical * scale).round() as u32;

    drawer
        .set_size(tauri::PhysicalSize::new(width, size.height))
        .map_err(|e| e.to_string())?;
    // Docked to the LEFT of the app, so the drawer reads as a sidebar rather
    // than something bolted on the end.
    drawer
        .set_position(tauri::PhysicalPosition::new(
            pos.x - width as i32 - gap,
            pos.y,
        ))
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn is_collapsed(drawer: &WebviewWindow, scale: f64) -> bool {
    drawer
        .outer_size()
        .map(|s| (s.width as f64 / scale) < (DRAWER_WIDTH + DRAWER_RAIL) / 2.0)
        .unwrap_or(false)
}

/// Collapse the drawer to a rail, or expand it back.
///
/// Deliberately NOT hide/show. A hidden drawer leaves the user with no visible way
/// back — the tray is not discoverable, and this is the panel that holds the undo.
/// Collapsed, it is a 26px strip with a chevron: always on screen, always clickable.
#[tauri::command]
async fn toggle_drawer(app: tauri::AppHandle) -> Result<bool, String> {
    let drawer = shell(&app)?;
    let scale = drawer.scale_factor().unwrap_or(1.0);
    let collapsing = !is_collapsed(&drawer, scale);

    if !drawer.is_visible().unwrap_or(false) {
        drawer.show().map_err(|e| e.to_string())?;
    }

    let _ = drawer.emit(if collapsing { "drawer:collapse" } else { "drawer:expand" }, ());
    if collapsing {
        // Let the content fade before the frame narrows, or the text reflows
        // visibly as it is clipped.
        tokio::time::sleep(Duration::from_millis(140)).await;
    }

    let width = ((if collapsing { DRAWER_RAIL } else { DRAWER_WIDTH }) * scale).round() as u32;
    let height = drawer.outer_size().map_err(|e| e.to_string())?.height;
    drawer
        .set_size(tauri::PhysicalSize::new(width, height))
        .map_err(|e| e.to_string())?;
    dock_drawer(&app)?;
    if !collapsing {
        let _ = drawer.set_focus();
    }
    Ok(!collapsing)
}

// ---------------------------------------------------------------- module store

/// Read the live source. Granted to the canvas too — reading its own source is
/// harmless, and it means the canvas boots itself rather than waiting to be fed.
#[tauri::command]
fn load_active_module(db: State<Db>, name: String) -> Result<String, String> {
    db.with(|c| {
        db::load_active(c, &name)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("no active version of module \"{name}\""))
    })
}

/// Append a version and make it live. Shell only — this is the write path the
/// canvas must never reach.
#[tauri::command]
fn save_module(
    db: State<Db>,
    name: String,
    source: String,
    note: Option<String>,
) -> Result<i64, String> {
    db.with(|c| db::save(c, &name, &source, note.as_deref()).map_err(|e| e.to_string()))
}

#[tauri::command]
fn list_versions(db: State<Db>, name: String) -> Result<Vec<db::VersionRow>, String> {
    db.with(|c| db::list_versions(c, &name).map_err(|e| e.to_string()))
}

#[tauri::command]
fn load_version(db: State<Db>, name: String, version: i64) -> Result<String, String> {
    db.with(|c| {
        db::load_version(c, &name, version)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("no version {version} of \"{name}\""))
    })
}

/// Move the live pointer to another version, in either direction.
///
/// Snapshots the data first — this is the boundary where a module can find a
/// shape it did not write, and where an older one can quietly strip a field a
/// newer one introduced.
#[tauri::command]
fn set_active_version(db: State<Db>, name: String, version: i64) -> Result<i64, String> {
    db.with(|c| {
        let from = db::active_version(c, &name).map_err(|e| e.to_string())?;
        let snapshot_id =
            db::snapshot_kv(c, from, version, "version switch").map_err(|e| e.to_string())?;
        db::set_active(c, &name, version)?;
        Ok(snapshot_id)
    })
}

// ------------------------------------------------------------------ app state

#[tauri::command]
fn kv_get(db: State<Db>, key: String) -> Result<Option<String>, String> {
    db.with(|c| db::kv_get(c, &key).map_err(|e| e.to_string()))
}

#[tauri::command]
fn kv_set(db: State<Db>, key: String, value: String) -> Result<(), String> {
    db.with(|c| db::kv_set(c, &key, &value))
}

/// The canvas registers what the live module claims to own, before it mounts.
///
/// This comes from untrusted code, and that is fine: declaring more fields only
/// lets a module destroy more of its *own* data — it cannot reach another key, and
/// the M5 snapshots still cover the case where it lies.
#[tauri::command]
fn declare_schema(db: State<Db>, schema: db::Schema) -> Result<(), String> {
    db.with(|c| {
        let version = db::active_version(c, db::ROOT_MODULE)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "no active module".to_string())?;
        db::declare_schema(c, version, &schema).map_err(|e| e.to_string())
    })
}

#[derive(Serialize)]
struct KeyStatus {
    key: String,
    protected: bool,
    shape: Option<String>,
}

/// Which app-data keys are covered by a declaration and which are written through
/// unprotected. Surfaced in the shell so unprotected data is visible, not implicit.
#[tauri::command]
fn schema_status(db: State<Db>) -> Result<Vec<KeyStatus>, String> {
    db.with(|c| {
        let version = db::active_version(c, db::ROOT_MODULE)
            .map_err(|e| e.to_string())?
            .unwrap_or(0);
        let schema = db::schema_for(c, version);
        let mut stmt = c
            .prepare("SELECT key FROM kv ORDER BY key")
            .map_err(|e| e.to_string())?;
        let keys: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .map_err(|e| e.to_string())?
            .collect::<rusqlite::Result<_>>()
            .map_err(|e| e.to_string())?;

        Ok(keys
            .into_iter()
            .map(|key| {
                let shape = schema.get(&key).map(|d| match d {
                    db::Declaration::Scalar => "scalar".to_string(),
                    db::Declaration::Record { .. } => "record".to_string(),
                    db::Declaration::RecordList { .. } => "record-list".to_string(),
                });
                KeyStatus { protected: shape.is_some(), key, shape }
            })
            .collect())
    })
}

// ------------------------------------------------------------------ the loop

#[tauri::command]
fn set_api_key(db: State<Db>, key: String) -> Result<(), String> {
    llm::set_key(&key)?;
    db.with(|c| db::set_setting(c, "api_key_saved", "1").map_err(|e| e.to_string()))
}

#[tauri::command]
fn clear_api_key(db: State<Db>) -> Result<(), String> {
    llm::clear_key()?;
    db.with(|c| db::set_setting(c, "api_key_saved", "0").map_err(|e| e.to_string()))
}

/// Says where a key would come from — never what it is. The key itself never
/// crosses into the webview; that is the whole reason the model call is in Rust.
///
/// Deliberately does NOT read the Keychain. Reading it is what triggers the macOS
/// authorisation prompt, and doing that on every launch to render a status dot is
/// what made the app nag. A plain marker in `settings` answers the question, and
/// the Keychain is touched only when a request is actually being made.
#[tauri::command]
fn api_key_status(db: State<Db>) -> Result<String, String> {
    if llm::env_key_present() {
        return Ok("env".into());
    }
    db.with(|c| {
        Ok(if db::get_setting(c, "api_key_saved", "0") == "1" {
            "keychain".to_string()
        } else {
            "none".to_string()
        })
    })
}

#[tauri::command]
fn chat_history(db: State<Db>) -> Result<Vec<db::ChatRow>, String> {
    db.with(|c| db::chat_history(c).map_err(|e| e.to_string()))
}

#[tauri::command]
fn link_chat_version(db: State<Db>, id: i64, version: i64) -> Result<(), String> {
    db.with(|c| db::chat_link_version(c, id, version).map_err(|e| e.to_string()))
}

#[derive(Serialize)]
struct ProposalReply {
    chat_id: i64,
    source: String,
    note: String,
    explanation: String,
}

/// Ask the model for a new module. Deliberately does NOT write it — the shell
/// applies via `save_module`, the same path the manual editor uses, so M5 has
/// exactly one place to install the verify-then-commit gate.
#[tauri::command]
async fn propose_change(app: tauri::AppHandle, prompt: String) -> Result<ProposalReply, String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("say what you want changed".into());
    }

    // Scoped so the database lock is released before the network call.
    let (source, history) = {
        let db = app.state::<Db>();
        db.with(|c| {
            let source = db::load_active(c, db::ROOT_MODULE)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "no active module".to_string())?;
            let history = db::chat_history(c)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|r| llm::Turn { role: r.role, content: r.content })
                .collect::<Vec<_>>();
            Ok((source, history))
        })?
    };

    let model = {
        let db = app.state::<Db>();
        db.with(|c| Ok(db::get_setting(c, "model", llm::DEFAULT_MODEL)))?
    };
    let key = llm::get_key()?;
    let proposal = llm::propose(&key, &model, &source, &history, &prompt).await?;

    let chat_id = {
        let db = app.state::<Db>();
        db.with(|c| {
            db::chat_append(c, "user", &prompt).map_err(|e| e.to_string())?;
            db::chat_append(c, "assistant", &proposal.explanation).map_err(|e| e.to_string())
        })?
    };

    Ok(ProposalReply {
        chat_id,
        source: proposal.source,
        note: proposal.note,
        explanation: proposal.explanation,
    })
}

#[derive(Serialize)]
struct BuildIdentity {
    /// Whether destructive dev tooling is present in this binary at all.
    dev: bool,
    /// Engine = the Rust binary, loader and shell. Its real integrity guarantee is
    /// the code signature, not this string.
    engine: String,
    /// App = the module history, as a hash chain head. Short form is the identity
    /// you read; a mismatch means a stored version was altered outside the app.
    app: Option<String>,
    ok: bool,
    versions: i64,
    broken_at: Option<i64>,
}

#[derive(Serialize)]
struct UpdateStatus {
    available: bool,
    version: Option<String>,
    notes: Option<String>,
    /// Set when the check could not run at all — no endpoint, offline, bad
    /// signature. Surfaced rather than swallowed: a silently failing updater is
    /// indistinguishable from an up-to-date one.
    error: Option<String>,
}

/// Ask the feed whether a newer engine exists. Does not install.
///
/// The engine is everything self-modification cannot reach: the Rust side, the
/// loader, the shell, the capability grants, the model contract. Modules and data
/// live in Application Support and are untouched by an engine update.
#[tauri::command]
async fn check_for_update(app: tauri::AppHandle) -> Result<UpdateStatus, String> {
    use tauri_plugin_updater::UpdaterExt;

    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            return Ok(UpdateStatus {
                available: false,
                version: None,
                notes: None,
                error: Some(e.to_string()),
            })
        }
    };

    match updater.check().await {
        Ok(Some(update)) => Ok(UpdateStatus {
            available: true,
            version: Some(update.version.clone()),
            notes: update.body.clone(),
            error: None,
        }),
        Ok(None) => Ok(UpdateStatus {
            available: false,
            version: None,
            notes: None,
            error: None,
        }),
        Err(e) => Ok(UpdateStatus {
            available: false,
            version: None,
            notes: None,
            error: Some(e.to_string()),
        }),
    }
}

/// Download and install, then restart. Deliberately a separate, explicit step —
/// this app is built on the idea that changes are reversible, and an engine
/// update is the one change that is not.
#[tauri::command]
async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_updater::UpdaterExt;

    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no update available".to_string())?;

    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;

    app.restart();
}

#[tauri::command]
fn build_identity(db: State<Db>) -> Result<BuildIdentity, String> {
    db.with(|c| {
        let report = db::verify_integrity(c, db::ROOT_MODULE).map_err(|e| e.to_string())?;
        Ok(BuildIdentity {
            dev: cfg!(debug_assertions),
            engine: env!("CARGO_PKG_VERSION").to_string(),
            app: report.head.map(|h| h[..8].to_string()),
            ok: report.ok,
            versions: report.versions,
            broken_at: report.broken_at,
        })
    })
}

#[derive(Serialize, Deserialize, Clone)]
struct CanvasTheme {
    #[serde(rename = "accentColor")]
    accent_color: String,
    #[serde(rename = "grayColor")]
    gray_color: String,
    radius: String,
    scaling: String,
    appearance: String,
}

impl Default for CanvasTheme {
    fn default() -> Self {
        Self {
            accent_color: "indigo".into(),
            gray_color: "slate".into(),
            radius: "medium".into(),
            scaling: "100%".into(),
            appearance: "inherit".into(),
        }
    }
}

/// Read by the canvas at boot. Granted to it — knowing its own theme is harmless.
#[tauri::command]
fn get_theme(db: State<Db>) -> Result<CanvasTheme, String> {
    db.with(|c| {
        Ok(serde_json::from_str(&db::get_setting(c, "theme", "")).unwrap_or_default())
    })
}

/// Store a theme and push it into the live canvas.
///
/// Pushed from Rust via `eval_with_callback` rather than a Tauri event, because
/// granting the canvas event access would let model-authored code emit
/// `drawer:request-close` and collapse the drawer — capturing the escape hatch
/// that `PRINCIPLES.md` says it must never be able to touch.
///
/// No reload: a theme change must not cost the user whatever they were typing.
#[tauri::command]
async fn set_theme(app: tauri::AppHandle, theme: CanvasTheme) -> Result<(), String> {
    let json = serde_json::to_string(&theme).map_err(|e| e.to_string())?;
    {
        let db = app.state::<Db>();
        db.with(|c| db::set_setting(c, "theme", &json).map_err(|e| e.to_string()))?;
    }
    let script = format!(
        "(() => {{ if (window.__set_theme) {{ window.__set_theme({json}); return true; }} return false; }})()"
    );
    canvas(&app)?.eval(&script).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_model(db: State<Db>) -> Result<String, String> {
    db.with(|c| Ok(db::get_setting(c, "model", llm::DEFAULT_MODEL)))
}

#[tauri::command]
fn set_model(db: State<Db>, model: String) -> Result<(), String> {
    if !llm::MODELS.contains(&model.as_str()) {
        return Err(format!("unknown model \"{model}\""));
    }
    db.with(|c| db::set_setting(c, "model", &model).map_err(|e| e.to_string()))
}

/// Reset to a version: truncate everything after it, clear app data, reload.
///
/// The destructive counterpart to `set_active_version`. Switching moves the
/// pointer and keeps both history and data; resetting throws away the branch you
/// are abandoning so the history stays a clean line.
///
/// This is the ONLY operation that deletes a version. It removes a suffix, so no
/// surviving hash is disturbed — see `db::reset_to`.
#[tauri::command]
async fn reset_to_version(
    app: tauri::AppHandle,
    name: String,
    version: i64,
    expected_head: Option<String>,
) -> Result<db::Reset, String> {
    let outcome = {
        let db = app.state::<Db>();
        db.with(|c| db::reset_to(c, &name, version, expected_head.as_deref()))?
    };
    canvas(&app)?.reload().map_err(|e| e.to_string())?;
    settle(&app, None, Duration::from_secs(6)).await;
    Ok(outcome)
}

#[tauri::command]
fn list_snapshots(db: State<Db>) -> Result<Vec<db::SnapshotRow>, String> {
    db.with(|c| db::list_snapshots(c).map_err(|e| e.to_string()))
}

#[tauri::command]
fn restore_snapshot(db: State<Db>, id: i64) -> Result<usize, String> {
    db.with(|c| {
        let live = db::active_version(c, db::ROOT_MODULE)
            .map_err(|e| e.to_string())?
            .unwrap_or(0);
        db::restore_snapshot(c, id, live)
    })
}

// -------------------------------------------------------------- canvas control

/// Ask the canvas how it is doing — from the privileged side.
///
/// The point of driving this from Rust rather than listening for an event the
/// canvas emits: the module under test does not get to vote on its own health.
/// If it wedged, threw, or never mounted, no event arrives and we time out,
/// which is itself the answer.
/// Health as the shell should read it: wait for a definite answer.
///
/// A raw single probe races the canvas. The shell refreshes on mount, catches a
/// half-loaded page, and reports "unhealthy" forever because nothing re-checks.
/// `settle` returns the moment the answer is real — mounted, or a definite
/// failure — so the common case is still immediate.
#[tauri::command]
async fn probe_canvas(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    Ok(settle(&app, None, Duration::from_secs(3)).await)
}

async fn probe_once(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let window = canvas(app)?;
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx = Mutex::new(Some(tx));

    window
        .eval_with_callback(
            r#"(() => {
                 try {
                   return window.__canvas_probe
                     ? window.__canvas_probe()
                     : { ok: false, mounted: false, nodeCount: 0, at: Date.now(),
                         error: "probe not installed" };
                 } catch (e) {
                   return { ok: false, mounted: false, nodeCount: 0, at: Date.now(),
                            error: String(e) };
                 }
               })()"#,
            move |result| {
                if let Ok(mut slot) = tx.lock() {
                    if let Some(tx) = slot.take() {
                        let _ = tx.send(result);
                    }
                }
            },
        )
        .map_err(|e| format!("eval failed: {e}"))?;

    let raw = tokio::time::timeout(Duration::from_secs(2), rx)
        .await
        .map_err(|_| "canvas did not answer within 2s — assume it is wedged".to_string())?
        .map_err(|e| format!("probe channel closed: {e}"))?;

    serde_json::from_str(&raw).map_err(|e| format!("bad probe payload: {e} (raw: {raw})"))
}

/// Poll until the canvas has actually settled on an answer.
///
/// Straight after a reload the probe is not installed yet — that is "still
/// loading", not "failed", and treating it as failure would roll back every
/// healthy swap. A module that mounted, or one that reported a definite
/// compile error, has settled. Anything still ambiguous at the deadline
/// (a render loop, a wedged process) is a failure by timeout.
async fn settle(
    app: &tauri::AppHandle,
    stale_boot: Option<&str>,
    budget: Duration,
) -> serde_json::Value {
    let deadline = std::time::Instant::now() + budget;
    let mut last = serde_json::json!({
        "ok": false, "mounted": false,
        "error": "canvas never settled", "phase": "timeout"
    });

    loop {
        if let Ok(report) = probe_once(app).await {
            let boot = report.get("boot").and_then(Value::as_str);
            // Ignore the outgoing document. It answers for a few milliseconds
            // after reload() and its verdict describes the module we replaced.
            let fresh = stale_boot.is_none() || (boot.is_some() && boot != stale_boot);
            let mounted = report.get("mounted").and_then(Value::as_bool).unwrap_or(false);
            let error = report.get("error").and_then(Value::as_str);
            let booted = error != Some("probe not installed");
            if fresh && (mounted || (booted && error.is_some())) {
                return report;
            }
            if fresh && booted {
                last = report;
            }
        }
        if std::time::Instant::now() >= deadline {
            return last;
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
}

#[derive(Serialize)]
struct ApplyOutcome {
    applied: bool,
    version: i64,
    rolled_back_to: Option<i64>,
    phase: Option<String>,
    error: Option<String>,
    snapshot_id: i64,
}

/// Save, swap, verify, and commit or undo — as one operation.
///
/// Lives in Rust rather than the shell so it cannot be left half-applied by a
/// closed window or a thrown promise. This is the single write path: the chat
/// and the manual editor both come through here.
#[tauri::command]
async fn apply_and_verify(
    app: tauri::AppHandle,
    name: String,
    source: String,
    note: String,
) -> Result<ApplyOutcome, String> {
    apply_inner(app, name, source, note, None).await
}

#[tauri::command]
fn list_templates() -> Vec<templates::TemplateInfo> {
    templates::list()
}

/// Start from a template. Goes through the same gate as everything else.
///
/// Branches rather than wipes: the template becomes a new version and existing
/// app data is left alone — schema grafting absorbs any shape mismatch. Sample
/// rows are seeded only into a genuinely empty store, so a fresh install looks
/// alive without ever mixing demo data into real work.
#[tauri::command]
async fn apply_template(app: tauri::AppHandle, id: String) -> Result<ApplyOutcome, String> {
    let template = templates::find(&id).ok_or_else(|| format!("no template \"{id}\""))?;
    apply_inner(
        app,
        db::ROOT_MODULE.to_string(),
        template.source.to_string(),
        format!("start from {} template", template.name),
        Some(template.sample),
    )
    .await
}

async fn apply_inner(
    app: tauri::AppHandle,
    name: String,
    source: String,
    note: String,
    sample: Option<&str>,
) -> Result<ApplyOutcome, String> {
    let (version, previous, snapshot_id) = {
        let db = app.state::<Db>();
        db.with(|c| {
            let previous = db::active_version(c, &name).map_err(|e| e.to_string())?;
            let version = db::save(c, &name, &source, Some(&note)).map_err(|e| e.to_string())?;
            let snapshot_id = db::snapshot_kv(c, previous, version, "module change")
                .map_err(|e| e.to_string())?;
            Ok((version, previous, snapshot_id))
        })?
    };

    // Captured before the swap so `settle` can tell the new page from the old.
    let stale_boot = probe_once(&app)
        .await
        .ok()
        .and_then(|r| r.get("boot").and_then(Value::as_str).map(str::to_owned));

    // Seeded after the version lands but before the canvas mounts, so the
    // template renders with its data rather than flashing empty first.
    if let Some(sample) = sample {
        let db = app.state::<Db>();
        db.with(|c| {
            // NOT "the store has no keys". The app boots a module that writes its
            // own empty key on mount, so by the time anyone picks a template the
            // store always has rows — just no content. What must never be
            // overwritten is real work, so the test is whether any key carries
            // anything.
            let untouched: bool = c
                .query_row(
                    "SELECT NOT EXISTS(SELECT 1 FROM kv WHERE value NOT IN ('[]', '{}', ''))",
                    [],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if !untouched {
                return Ok(());
            }
            let parsed: serde_json::Value =
                serde_json::from_str(sample).map_err(|e| e.to_string())?;
            if let Some(rows) = parsed.as_object() {
                for (key, value) in rows {
                    db::kv_set(c, key, &value.to_string())?;
                }
            }
            Ok(())
        })?;
    }

    canvas(&app)?.reload().map_err(|e| e.to_string())?;
    let report = settle(&app, stale_boot.as_deref(), Duration::from_secs(6)).await;
    let healthy = report.get("ok").and_then(Value::as_bool).unwrap_or(false);

    if healthy {
        return Ok(ApplyOutcome {
            applied: true,
            version,
            rolled_back_to: None,
            phase: None,
            error: None,
            snapshot_id,
        });
    }

    let phase = report.get("phase").and_then(Value::as_str).map(str::to_owned);
    let error = report.get("error").and_then(Value::as_str).map(str::to_owned);

    // A Sucrase parse failure carries a full stack trace. Useful in the probe,
    // unreadable as a chat bubble — keep the first line, which is the diagnosis.
    let headline = error
        .as_deref()
        .map(|e| {
            let first = e.lines().next().unwrap_or(e).trim();
            if first.len() > 180 { format!("{}…", &first[..180]) } else { first.to_string() }
        })
        .unwrap_or_else(|| "no detail".to_string());

    // The new version stays in the table — it is evidence, and the user may
    // want to read it. Only the pointer goes back.
    let rolled_back_to = match previous {
        Some(prev) => {
            let db = app.state::<Db>();
            db.with(|c| {
                db::snapshot_kv(c, Some(version), prev, "auto rollback")
                    .map_err(|e| e.to_string())?;
                db::mark_failed(c, &name, version).map_err(|e| e.to_string())?;
                db::set_active(c, &name, prev)?;
                // Recorded so the next prompt carries the failure — the model
                // sees what it broke without the user retyping it.
                db::chat_append(
                    c,
                    "system",
                    &format!(
                        "v{version} failed at {} — {headline}. Switched back to v{prev}.",
                        phase.as_deref().unwrap_or("startup")
                    ),
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            })?;
            let failed_boot = report.get("boot").and_then(Value::as_str).map(str::to_owned);
            canvas(&app)?.reload().map_err(|e| e.to_string())?;
            settle(&app, failed_boot.as_deref(), Duration::from_secs(6)).await;
            Some(prev)
        }
        None => None,
    };

    Ok(ApplyOutcome {
        applied: false,
        version,
        rolled_back_to,
        phase,
        error,
        snapshot_id,
    })
}

#[tauri::command]
async fn reload_canvas(app: tauri::AppHandle) -> Result<(), String> {
    canvas(&app)?.reload().map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // CLAYA_DB lets the e2e suite run against a throwaway database
            // instead of the user's real history.
            let path = match std::env::var("CLAYA_DB") {
                Ok(p) if !p.trim().is_empty() => std::path::PathBuf::from(p),
                _ => {
                    let dir = app.path().app_data_dir()?;
                    std::fs::create_dir_all(&dir)?;
                    dir.join("claya.db")
                }
            };
            let conn = db::open(&path)?;
            db::ensure_seeded(&conn)?;
            println!("[db] {}", path.display());
            // A rename strands the Keychain item under the old service name, and
            // the shell's key gate would block the user from ever triggering the
            // lazy fallback. Adopt it up front instead.
            if cfg!(debug_assertions) && !llm::env_key_present() {
                println!(
                    "[key] using the Keychain. Every rebuild is a new code signature, so\n\
                     [key] macOS will ask again after each one. To stop that for good:\n\
                     [key]     export ANTHROPIC_API_KEY=sk-ant-..."
                );
            }
            match db::verify_integrity(&conn, db::ROOT_MODULE) {
                Ok(r) => println!(
                    "[integrity] ok={} versions={} head={} broken_at={:?}",
                    r.ok,
                    r.versions,
                    r.head.as_deref().map(|h| &h[..8]).unwrap_or("-"),
                    r.broken_at
                ),
                Err(e) => println!("[integrity] FAILED: {e}"),
            }
            app.manage(Db(Mutex::new(conn)));

            // Startup self-test: both windows exist, the canvas pulled its
            // source from SQLite and mounted it, and the ACL is denying the
            // canvas its privileged-command attempt.
            // The drawer follows the canvas. Done in Rust because only the
            // privileged side is allowed to know both windows exist.
            let docker = app.handle().clone();
            if let Some(win) = app.get_webview_window(CANVAS) {
                win.on_window_event(move |event| {
                    if matches!(event, WindowEvent::Moved(_) | WindowEvent::Resized(_)) {
                        let _ = dock_drawer(&docker);
                    }
                });
            }
            let _ = dock_drawer(app.handle());

            // Tray toggle. Deliberately NOT a button inside the canvas: that window
            // renders model-authored code, and the control you use to undo a bad
            // generation must not be something a bad generation can hide.
            let tray_handle = app.handle().clone();
            let toggle = tauri::menu::MenuItem::with_id(app, "toggle", "Collapse / expand chat", true, None::<&str>)?;
            let quit = tauri::menu::MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = tauri::menu::Menu::with_items(app, &[&toggle, &quit])?;
            tauri::tray::TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(move |_app, event| {
                    let handle = tray_handle.clone();
                    match event.id().as_ref() {
                        "toggle" => {
                            tauri::async_runtime::spawn(async move {
                                let _ = toggle_drawer(handle).await;
                            });
                        }
                        "quit" => handle.exit(0),
                        _ => {}
                    }
                })
                .build(app)?;

            // The drawer asks to be closed from its own header chevron.
            let closer = app.handle().clone();
            app.listen_any("drawer:request-close", move |_| {
                let handle = closer.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = toggle_drawer(handle).await;
                });
            });

            let handle = app.handle().clone();
            let handle_exit = app.handle().clone();
            let handle_tpl = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Adopting a pre-rename Keychain item opens a macOS password
                // dialog. Doing that on the launch path blocked startup behind a
                // modal with no windows drawn yet — so it happens here, after the
                // UI exists and the user can see what is asking.
                {
                    let db = handle.state::<Db>();
                    let recorded = db
                        .with(|c| Ok(db::get_setting(c, "api_key_saved", "0")))
                        .unwrap_or_else(|_| "0".into());
                    if recorded != "1" && !llm::env_key_present() && llm::adopt_legacy_key() {
                        let _ = db.with(|c| {
                            db::set_setting(c, "api_key_saved", "1").map_err(|e| e.to_string())
                        });
                    }
                }

                println!("[self-test] {}", settle(&handle, None, Duration::from_secs(10)).await);

                // Opt-in: prove the M5 gate actually fires by shipping a module
                // that cannot compile and checking we end up back where we were.
                // Leaves a real failed version in the history, which is the point.
                if std::env::var("CLAYA_VERIFY_ROLLBACK").is_ok() {
                    let before = {
                        let db = handle.state::<Db>();
                        db.with(|c| {
                            db::active_version(c, db::ROOT_MODULE).map_err(|e| e.to_string())
                        })
                        .ok()
                        .flatten()
                    };
                    let broken = "export default function App() { return <div>unclosed }";
                    match apply_and_verify(
                        handle.clone(),
                        db::ROOT_MODULE.to_string(),
                        broken.to_string(),
                        "self-test: deliberate failure".to_string(),
                    )
                    .await
                    {
                        Ok(o) => {
                            let restored = o.rolled_back_to == before;
                            println!(
                                "[gate-test] applied={} phase={:?} rolled_back_to={:?} was={:?} restored={}",
                                o.applied, o.phase, o.rolled_back_to, before, restored
                            );
                            println!(
                                "[gate-test] after rollback: {}",
                                settle(&handle, None, Duration::from_secs(6)).await
                            );
                        }
                        Err(e) => println!("[gate-test] FAILED: {e}"),
                    }
                }

                // Opt-in: run every shipped template through the real gate.
                // Unit tests prove they compile; this proves they mount.
                if std::env::var("CLAYA_VERIFY_TEMPLATES").is_ok() {
                    for t in templates::TEMPLATES {
                        match apply_template(handle_tpl.clone(), t.id.to_string()).await {
                            Ok(o) => println!(
                                "[template-test] {} applied={} v{} phase={:?}",
                                t.id, o.applied, o.version, o.phase
                            ),
                            Err(e) => println!("[template-test] {} FAILED: {e}", t.id),
                        }
                    }
                    // Every template writes its own (empty) key when it mounts,
                    // so a raw key count says nothing. What matters is that
                    // sample ROWS landed exactly once — into the empty store the
                    // first template saw, and never again.
                    let populated = {
                        let db = handle_tpl.state::<Db>();
                        db.with(|c| {
                            c.query_row(
                                "SELECT COUNT(*) FROM kv WHERE value NOT IN ('[]', '{}', '')",
                                [],
                                |r| r.get::<_, i64>(0),
                            )
                            .map_err(|e| e.to_string())
                        })
                        .unwrap_or(-1)
                    };
                    println!("[template-test] keys carrying sample rows: {populated}");
                }

                // Drive one full turn of the loop from the command line:
                //
                //   CLAYA_PROMPT="make the background black" npm run tauri dev
                //
                // Goes through propose_change -> apply_and_verify, the exact path
                // the chat box uses, so it exercises the model call, the contract,
                // the gate and the rollback together. Costs a real API call.
                // CLAYA_RESET=N truncates to vN first, so a scripted rebuild
                // starts from a clean line instead of piling onto the old one.
                if let Ok(target) = std::env::var("CLAYA_RESET") {
                    if let Ok(version) = target.trim().parse::<i64>() {
                        // Scripted resets pass no head: the operator asked for this
                        // explicitly and has no screen to have gone stale.
                        match reset_to_version(handle_tpl.clone(), db::ROOT_MODULE.to_string(), version, None).await {
                            Ok(r) => println!(
                                "[reset] to v{} — {} removed, {} keys cleared, head={}",
                                r.active,
                                r.removed.len(),
                                r.cleared_keys,
                                r.head.as_deref().map(|h| &h[..8]).unwrap_or("-")
                            ),
                            Err(e) => println!("[reset] FAILED: {e}"),
                        }
                    }
                }

                // Multiple prompts separated by " || " run in order, each becoming
                // its own version — one app run, one Keychain prompt.
                if let Ok(script) = std::env::var("CLAYA_PROMPT") {
                    for prompt in script.split(" || ").map(str::trim).filter(|p| !p.is_empty()) {
                        let prompt = prompt.to_string();
                        println!("[prompt] > {prompt}");
                        match propose_change(handle_tpl.clone(), prompt).await {
                            Ok(p) => {
                                println!("[prompt] note: {}", p.note);
                                println!("[prompt] says: {}", p.explanation);
                                match apply_and_verify(
                                    handle_tpl.clone(),
                                    db::ROOT_MODULE.to_string(),
                                    p.source,
                                    p.note,
                                )
                                .await
                                {
                                    Ok(o) => println!(
                                        "[prompt] applied={} v{} phase={:?} error={:?}",
                                        o.applied, o.version, o.phase, o.error
                                    ),
                                    Err(e) => println!("[prompt] apply FAILED: {e}"),
                                }
                            }
                            Err(e) => println!("[prompt] FAILED: {e}"),
                        }
                    }
                }

                if std::env::var("CLAYA_EXIT_AFTER_TEST").is_ok() {
                    handle_exit.exit(0);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_active_module,
            save_module,
            list_versions,
            load_version,
            set_active_version,
            kv_get,
            kv_set,
            declare_schema,
            schema_status,
            set_api_key,
            clear_api_key,
            api_key_status,
            chat_history,
            link_chat_version,
            propose_change,
            apply_and_verify,
            list_templates,
            apply_template,
            build_identity,
            get_theme,
            set_theme,
            check_for_update,
            install_update,
            get_model,
            set_model,
            reset_to_version,
            list_snapshots,
            restore_snapshot,
            probe_canvas,
            reload_canvas,
            toggle_drawer
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
