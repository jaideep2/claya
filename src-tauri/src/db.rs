//! Module storage with version history.
//!
//! Every edit is an INSERT, never an UPDATE. `active_module` is a pointer to
//! whichever version is live, so rollback is a single pointer move and no
//! version is ever destroyed — which is the property the whole safety story in
//! M5 rests on.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Compiled in, so a fresh install has something to run before any model call.
pub const SEED_SOURCE: &str = include_str!("../../src/canvas/seed-modules/todo.seed.tsx");
pub const ROOT_MODULE: &str = "app";

/// Cap on a single kv value. Model-authored code writes here; an unbounded
/// blob store one prompt away from "cache the whole list in one key" is not one.
const MAX_VALUE_BYTES: usize = 256 * 1024;

#[derive(Serialize)]
pub struct VersionRow {
    pub version: i64,
    pub note: Option<String>,
    pub created_at: i64,
    pub bytes: i64,
    pub active: bool,
    pub failed: bool,
}

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
    migrate(&conn)?;
    backfill_hashes(&conn, ROOT_MODULE)?;
    Ok(conn)
}

/// Schema for the engine itself. Distinct from the model-facing migration DSL
/// in M6 — this one is ours and the model never touches it.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current < 1 {
        conn.execute_batch(
            r#"
            BEGIN;
            CREATE TABLE modules (
              name           TEXT    NOT NULL,
              version        INTEGER NOT NULL,
              source         TEXT    NOT NULL,
              parent_version INTEGER,
              note           TEXT,
              created_at     INTEGER NOT NULL,
              PRIMARY KEY (name, version)
            );
            CREATE TABLE active_module (
              name    TEXT PRIMARY KEY,
              version INTEGER NOT NULL
            );
            CREATE TABLE kv (
              key        TEXT PRIMARY KEY,
              value      TEXT NOT NULL,
              updated_at INTEGER NOT NULL
            );
            PRAGMA user_version = 1;
            COMMIT;
            "#,
        )?;
    }
    if current < 2 {
        conn.execute_batch(
            r#"
            BEGIN;
            CREATE TABLE chat (
              id         INTEGER PRIMARY KEY AUTOINCREMENT,
              role       TEXT    NOT NULL,
              content    TEXT    NOT NULL,
              version    INTEGER,
              created_at INTEGER NOT NULL
            );
            PRAGMA user_version = 2;
            COMMIT;
            "#,
        )?;
    }
    if current < 3 {
        conn.execute_batch(
            r#"
            BEGIN;
            CREATE TABLE kv_snapshots (
              id           INTEGER PRIMARY KEY AUTOINCREMENT,
              from_version INTEGER,
              to_version   INTEGER NOT NULL,
              reason       TEXT    NOT NULL,
              taken_at     INTEGER NOT NULL
            );
            CREATE TABLE kv_snapshot_entries (
              snapshot_id INTEGER NOT NULL REFERENCES kv_snapshots(id),
              key         TEXT    NOT NULL,
              value       TEXT    NOT NULL,
              PRIMARY KEY (snapshot_id, key)
            );
            CREATE TABLE settings (
              key   TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );
            PRAGMA user_version = 3;
            COMMIT;
            "#,
        )?;
    }
    if current < 4 {
        // Backfill the key marker for anyone who already stored one. A prior
        // successful exchange proves a key exists, so we can infer it without
        // reading the Keychain — which is the thing that prompts.
        conn.execute_batch(
            r#"
            BEGIN;
            INSERT INTO settings (key, value)
            SELECT 'api_key_saved', '1' WHERE EXISTS (SELECT 1 FROM chat)
            ON CONFLICT(key) DO NOTHING;
            PRAGMA user_version = 4;
            COMMIT;
            "#,
        )?;
    }
    if current < 5 {
        conn.execute_batch(
            r#"
            BEGIN;
            CREATE TABLE module_schemas (
              version     INTEGER PRIMARY KEY,
              schema_json TEXT    NOT NULL,
              declared_at INTEGER NOT NULL
            );
            PRAGMA user_version = 5;
            COMMIT;
            "#,
        )?;
    }
    if current < 6 {
        conn.execute_batch(
            r#"
            BEGIN;
            ALTER TABLE modules ADD COLUMN failed INTEGER NOT NULL DEFAULT 0;
            PRAGMA user_version = 6;
            COMMIT;
            "#,
        )?;
    }
    if current < 7 {
        conn.execute_batch(
            r#"
            BEGIN;
            -- These rows were created by the gate self-test and every one of them
            -- was rolled back, so marking them is accurate rather than cosmetic.
            UPDATE modules SET failed = 1 WHERE note = 'self-test: deliberate failure';
            PRAGMA user_version = 7;
            COMMIT;
            "#,
        )?;
    }
    if current < 8 {
        conn.execute_batch(
            r#"
            BEGIN;
            ALTER TABLE modules ADD COLUMN hash TEXT;
            PRAGMA user_version = 8;
            COMMIT;
            "#,
        )?;
    }
    if current < 9 {
        // Reclassify, never delete — these rows are real history and
        // `PRINCIPLES.md` does not make an exception for ugly history. A gate
        // rollback is a system event, not something the model said, and styling
        // it as an assistant reply let six of them dominate the transcript.
        conn.execute_batch(
            r#"
            BEGIN;
            UPDATE chat SET role = 'system'
             WHERE role = 'assistant' AND content LIKE 'v_% failed at %';
            PRAGMA user_version = 9;
            COMMIT;
            "#,
        )?;
    }
    Ok(())
}

// ------------------------------------------------------------------ integrity

/// Hash chain over the module history.
///
/// Each version's hash folds in its parent's, so the newest hash is a fingerprint
/// of the *entire* history. Editing v3 with a sqlite3 shell changes v3's hash and
/// therefore every hash after it — the app notices on next launch.
///
/// This deliberately covers module history only. Binary integrity is codesign's
/// job and the OS does it better; app *data* changes constantly and hashing it
/// would just produce noise.
pub fn hash_version(
    parent_hash: Option<&str>,
    name: &str,
    version: i64,
    source: &str,
    note: Option<&str>,
) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    // Length-prefixed so no field can impersonate a boundary.
    for field in [
        parent_hash.unwrap_or(""),
        name,
        &version.to_string(),
        source,
        note.unwrap_or(""),
    ] {
        h.update(field.len().to_le_bytes());
        h.update(field.as_bytes());
    }
    hex::encode(h.finalize())
}

#[derive(Serialize)]
pub struct Integrity {
    pub ok: bool,
    /// Chain head — short form is the app's identity.
    pub head: Option<String>,
    pub versions: i64,
    /// First version whose stored hash disagrees with a recomputation.
    pub broken_at: Option<i64>,
}

fn chain_rows(conn: &Connection, name: &str) -> rusqlite::Result<Vec<(i64, String, Option<String>, Option<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT version, source, note, hash FROM modules WHERE name = ?1 ORDER BY version ASC",
    )?;
    let rows = stmt.query_map([name], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    rows.collect()
}

/// Recompute the chain and compare against what is stored.
pub fn verify_integrity(conn: &Connection, name: &str) -> rusqlite::Result<Integrity> {
    let rows = chain_rows(conn, name)?;
    let mut parent: Option<String> = None;
    let mut broken_at = None;

    for (version, source, note, stored) in &rows {
        let expected = hash_version(parent.as_deref(), name, *version, source, note.as_deref());
        if broken_at.is_none() && stored.as_deref() != Some(expected.as_str()) {
            broken_at = Some(*version);
        }
        parent = Some(expected);
    }

    Ok(Integrity {
        ok: broken_at.is_none() && !rows.is_empty(),
        head: parent,
        versions: rows.len() as i64,
        broken_at,
    })
}

#[derive(Serialize, Debug)]
pub struct Reset {
    pub removed: Vec<i64>,
    pub active: i64,
    pub cleared_keys: usize,
    pub head: Option<String>,
    pub snapshot_id: i64,
}

/// Reset to a version: truncate everything after it and clear app data.
///
/// The ONLY operation that deletes a version. Distinct from `set_active`
/// (switch), which moves the pointer and leaves both history and data alone.
/// Reset is the destructive one, deliberately: it exists to get back to a clean
/// line, which is impossible while the abandoned branch sits in the list.
///
/// - **Versions after the target are gone.** Not archived, not hidden.
/// - **`kv` is cleared**, so the next version starts from nothing.
/// - **The chain is NOT rewritten, and must not be.** Truncation removes a
///   suffix, and no surviving hash depends on a row after it — `h1` was computed
///   from `(∅, name, 1, source, note)` and stays byte-identical however many
///   later versions go. This is `git reset --hard`: the commits that remain keep
///   their SHAs, only HEAD moves. Because middle-removal is not offered at all,
///   **no code path in this app ever overwrites a hash it has already written**,
///   and tamper-evidence is unconditional.
///
/// One thing IS kept: the data is snapshotted first, so a reset fired by mistake
/// can be undone from the snapshots panel. `PRINCIPLES.md` requires that of every
/// destructive action, and most of all of the ones that exist to destroy.
pub fn reset_to(
    conn: &Connection,
    name: &str,
    version: i64,
    expected_head: Option<&str>,
) -> Result<Reset, String> {
    // Compare-and-swap on the chain head.
    //
    // The confirm button says "delete N versions", but N was computed the last
    // time the shell refreshed. If a generation landed since — from this window,
    // another window, or a scripted run — the real N is larger and the user is
    // agreeing to something they were never shown. Refuse rather than delete more
    // than was on screen.
    if let Some(expected) = expected_head {
        let current = verify_integrity(conn, name).map_err(|e| e.to_string())?.head;
        let matches = current
            .as_deref()
            .map(|h| h.starts_with(expected))
            .unwrap_or(false);
        if !matches {
            return Err(format!(
                "history moved since you looked (head is {}, you saw {expected}) — \
                 refresh and check what would be deleted",
                current.as_deref().map(|h| &h[..8]).unwrap_or("-")
            ));
        }
    }

    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM modules WHERE name = ?1 AND version = ?2)",
            params![name, version],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Err(format!("no version {version} of \"{name}\""));
    }

    let snapshot_id =
        snapshot_kv(conn, active_version(conn, name).ok().flatten(), version, "before reset")
            .map_err(|e| e.to_string())?;

    let removed: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT version FROM modules WHERE name = ?1 AND version > ?2 ORDER BY version")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![name, version], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<rusqlite::Result<_>>().map_err(|e| e.to_string())?
    };

    for v in &removed {
        conn.execute("DELETE FROM module_schemas WHERE version = ?1", params![v])
            .map_err(|e| e.to_string())?;
    }
    conn.execute(
        "DELETE FROM modules WHERE name = ?1 AND version > ?2",
        params![name, version],
    )
    .map_err(|e| e.to_string())?;

    let cleared_keys = conn.execute("DELETE FROM kv", []).map_err(|e| e.to_string())?;

    conn.execute(
        "UPDATE active_module SET version = ?2 WHERE name = ?1",
        params![name, version],
    )
    .map_err(|e| e.to_string())?;

    // Deliberately no re-stamp. If integrity does not verify after a truncation,
    // something is wrong that should surface — not be papered over by recomputing
    // the hashes until they agree with whatever is on disk.
    let report = verify_integrity(conn, name).map_err(|e| e.to_string())?;
    if !report.ok {
        return Err(format!(
            "reset left the chain broken at v{:?} — refusing to hide it by re-stamping",
            report.broken_at
        ));
    }

    Ok(Reset { removed, active: version, cleared_keys, head: report.head, snapshot_id })
}

/// Stamp any rows written before hashing existed. Run once at open.
pub fn backfill_hashes(conn: &Connection, name: &str) -> rusqlite::Result<usize> {
    let rows = chain_rows(conn, name)?;
    let mut parent: Option<String> = None;
    let mut written = 0;
    for (version, source, note, stored) in &rows {
        let expected = hash_version(parent.as_deref(), name, *version, source, note.as_deref());
        if stored.is_none() {
            conn.execute(
                "UPDATE modules SET hash = ?3 WHERE name = ?1 AND version = ?2",
                params![name, version, expected],
            )?;
            written += 1;
        }
        parent = Some(expected);
    }
    Ok(written)
}

/// Mark a version the gate rejected. It stays in the table and stays switchable —
/// it is evidence, and sometimes worth reading — but the history should not present
/// it as an equal candidate.
pub fn mark_failed(conn: &Connection, name: &str, version: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE modules SET failed = 1 WHERE name = ?1 AND version = ?2",
        params![name, version],
    )?;
    Ok(())
}

// ------------------------------------------------------- schema (M6)

/// What a module claims to own.
///
/// The rule the whole milestone reduces to: **a module may only destroy fields it
/// declares.** Anything it does not declare belongs to some other version and is
/// carried through untouched, so an older module writing back cannot silently drop
/// a field a newer one introduced.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Declaration {
    /// Opaque value. Replaced wholesale.
    Scalar,
    /// A single object.
    Record { fields: Vec<String> },
    /// An array of objects identified by `key`.
    RecordList { key: String, fields: Vec<String> },
}

pub type Schema = std::collections::HashMap<String, Declaration>;

pub fn declare_schema(conn: &Connection, version: i64, schema: &Schema) -> rusqlite::Result<()> {
    let json = serde_json::to_string(schema).unwrap_or_else(|_| "{}".into());
    conn.execute(
        "INSERT INTO module_schemas (version, schema_json, declared_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(version) DO UPDATE SET schema_json = excluded.schema_json,
                                            declared_at = excluded.declared_at",
        params![version, json, now_ms()],
    )?;
    Ok(())
}

pub fn schema_for(conn: &Connection, version: i64) -> Schema {
    conn.query_row(
        "SELECT schema_json FROM module_schemas WHERE version = ?1",
        [version],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|j| serde_json::from_str(&j).ok())
    .unwrap_or_default()
}

fn key_of(value: &Value, field: &str) -> Option<String> {
    value.get(field).map(|k| match k {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// Copy across every stored field the writer did not declare and did not supply.
fn graft(stored: &Value, mut incoming: Value, owned: &[String]) -> Value {
    let Some(previous) = stored.as_object() else { return incoming };
    let Some(next) = incoming.as_object_mut() else { return incoming };
    for (field, value) in previous {
        if !owned.iter().any(|f| f == field) && !next.contains_key(field) {
            next.insert(field.clone(), value.clone());
        }
    }
    incoming
}

pub fn merge_preserving_unknown(decl: &Declaration, stored: &Value, incoming: Value) -> Value {
    match decl {
        Declaration::Scalar => incoming,
        Declaration::Record { fields } => graft(stored, incoming, fields),
        Declaration::RecordList { key, fields } => {
            let previous: std::collections::HashMap<String, &Value> = stored
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .filter_map(|r| key_of(r, key).map(|k| (k, r)))
                        .collect()
                })
                .unwrap_or_default();

            let Some(rows) = incoming.as_array() else { return incoming };
            Value::Array(
                rows.iter()
                    .map(|row| match key_of(row, key).and_then(|k| previous.get(&k).copied()) {
                        // A row the writer dropped from the array is an intentional
                        // delete and stays deleted. Only surviving rows are grafted.
                        Some(prev) => graft(prev, row.clone(), fields),
                        None => row.clone(),
                    })
                    .collect(),
            )
        }
    }
}

// ------------------------------------------------------------------ settings

pub fn get_setting(conn: &Connection, key: &str, fallback: &str) -> String {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
        r.get::<_, String>(0)
    })
    .unwrap_or_else(|_| fallback.to_string())
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

// ----------------------------------------------------------- data at the seam

#[derive(Serialize)]
pub struct SnapshotRow {
    pub id: i64,
    pub from_version: Option<i64>,
    pub to_version: i64,
    pub reason: String,
    pub taken_at: i64,
    pub keys: i64,
}

/// Copy the whole kv store aside before the module pointer moves.
///
/// `kv` stays shared and live across versions on purpose — that is what lets a
/// field a newer module introduced survive into the next one. The cost of
/// sharing is that an older module can quietly strip a field it does not know
/// about when it writes back. This snapshot is the recovery for that: bounded
/// by the number of version changes rather than the number of writes, and taken
/// at exactly the boundary where the shape can change underneath the data.
pub fn snapshot_kv(
    conn: &Connection,
    from_version: Option<i64>,
    to_version: i64,
    reason: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO kv_snapshots (from_version, to_version, reason, taken_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![from_version, to_version, reason, now_ms()],
    )?;
    let id = conn.last_insert_rowid();
    conn.execute(
        "INSERT INTO kv_snapshot_entries (snapshot_id, key, value)
         SELECT ?1, key, value FROM kv",
        params![id],
    )?;
    Ok(id)
}

pub fn list_snapshots(conn: &Connection) -> rusqlite::Result<Vec<SnapshotRow>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.from_version, s.to_version, s.reason, s.taken_at,
                (SELECT COUNT(*) FROM kv_snapshot_entries e WHERE e.snapshot_id = s.id)
           FROM kv_snapshots s ORDER BY s.id DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(SnapshotRow {
            id: r.get(0)?,
            from_version: r.get(1)?,
            to_version: r.get(2)?,
            reason: r.get(3)?,
            taken_at: r.get(4)?,
            keys: r.get(5)?,
        })
    })?;
    rows.collect()
}

/// Put the data back the way it was at a snapshot.
///
/// Snapshots the *current* state first, so a restore is itself undoable and the
/// "nothing is ever destroyed" rule survives the recovery path too.
pub fn restore_snapshot(conn: &Connection, snapshot_id: i64, live_version: i64) -> Result<usize, String> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM kv_snapshots WHERE id = ?1)",
            [snapshot_id],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Err(format!("no snapshot {snapshot_id}"));
    }

    snapshot_kv(conn, Some(live_version), live_version, "before restore")
        .map_err(|e| e.to_string())?;

    conn.execute("DELETE FROM kv", [])
        .map_err(|e| e.to_string())?;
    let restored = conn
        .execute(
            "INSERT INTO kv (key, value, updated_at)
             SELECT key, value, ?2 FROM kv_snapshot_entries WHERE snapshot_id = ?1",
            params![snapshot_id, now_ms()],
        )
        .map_err(|e| e.to_string())?;
    Ok(restored)
}

#[derive(Serialize, Deserialize)]
pub struct ChatRow {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub version: Option<i64>,
    pub created_at: i64,
}

pub fn chat_append(
    conn: &Connection,
    role: &str,
    content: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO chat (role, content, created_at) VALUES (?1, ?2, ?3)",
        params![role, content, now_ms()],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Marks which module version a proposal actually produced, once it is applied.
/// Left NULL for a proposal the user discarded — the transcript keeps the turn
/// either way, so the model can see what was tried and rejected.
pub fn chat_link_version(conn: &Connection, id: i64, version: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE chat SET version = ?2 WHERE id = ?1",
        params![id, version],
    )?;
    Ok(())
}

pub fn chat_history(conn: &Connection) -> rusqlite::Result<Vec<ChatRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, role, content, version, created_at FROM chat ORDER BY id ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(ChatRow {
            id: r.get(0)?,
            role: r.get(1)?,
            content: r.get(2)?,
            version: r.get(3)?,
            created_at: r.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn ensure_seeded(conn: &Connection) -> rusqlite::Result<()> {
    let seeded: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM active_module WHERE name = ?1)",
        [ROOT_MODULE],
        |r| r.get(0),
    )?;
    if !seeded {
        save(conn, ROOT_MODULE, SEED_SOURCE, Some("seed"))?;
    }
    Ok(())
}

/// Append a new version and point `active_module` at it. Returns the version.
pub fn save(
    conn: &Connection,
    name: &str,
    source: &str,
    note: Option<&str>,
) -> rusqlite::Result<i64> {
    let parent: Option<i64> = conn
        .query_row(
            "SELECT version FROM active_module WHERE name = ?1",
            [name],
            |r| r.get(0),
        )
        .optional()?;

    let next: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM modules WHERE name = ?1",
        [name],
        |r| r.get(0),
    )?;

    // Chain over insertion order: this is a tamper-evident log, so the link is
    // to the previous row written, not to `parent_version` (which records the
    // lineage a rollback-then-edit produced).
    let prior_hash: Option<String> = conn
        .query_row(
            "SELECT hash FROM modules WHERE name = ?1 ORDER BY version DESC LIMIT 1",
            [name],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let hash = hash_version(prior_hash.as_deref(), name, next, source, note);

    conn.execute(
        "INSERT INTO modules (name, version, source, parent_version, note, created_at, hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![name, next, source, parent, note, now_ms(), hash],
    )?;
    conn.execute(
        "INSERT INTO active_module (name, version) VALUES (?1, ?2)
         ON CONFLICT(name) DO UPDATE SET version = excluded.version",
        params![name, next],
    )?;
    Ok(next)
}

pub fn active_version(conn: &Connection, name: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT version FROM active_module WHERE name = ?1",
        [name],
        |r| r.get(0),
    )
    .optional()
}

pub fn load_active(conn: &Connection, name: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT m.source
           FROM modules m
           JOIN active_module a ON a.name = m.name AND a.version = m.version
          WHERE m.name = ?1",
        [name],
        |r| r.get(0),
    )
    .optional()
}

pub fn load_version(conn: &Connection, name: &str, version: i64) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT source FROM modules WHERE name = ?1 AND version = ?2",
        params![name, version],
        |r| r.get(0),
    )
    .optional()
}

pub fn list_versions(conn: &Connection, name: &str) -> rusqlite::Result<Vec<VersionRow>> {
    let active: Option<i64> = conn
        .query_row(
            "SELECT version FROM active_module WHERE name = ?1",
            [name],
            |r| r.get(0),
        )
        .optional()?;

    let mut stmt = conn.prepare(
        "SELECT version, note, created_at, LENGTH(source), failed
           FROM modules WHERE name = ?1 ORDER BY version DESC",
    )?;
    let rows = stmt.query_map([name], |r| {
        let version: i64 = r.get(0)?;
        Ok(VersionRow {
            version,
            note: r.get(1)?,
            created_at: r.get(2)?,
            bytes: r.get(3)?,
            active: Some(version) == active,
            failed: r.get::<_, i64>(4)? != 0,
        })
    })?;
    rows.collect()
}

/// Rollback. Moves the pointer only — the version being left behind stays.
pub fn set_active(conn: &Connection, name: &str, version: i64) -> Result<(), String> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM modules WHERE name = ?1 AND version = ?2)",
            params![name, version],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if !exists {
        return Err(format!("no version {version} of module \"{name}\""));
    }
    conn.execute(
        "UPDATE active_module SET version = ?2 WHERE name = ?1",
        params![name, version],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn kv_get(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM kv WHERE key = ?1", [key], |r| r.get(0))
        .optional()
}

pub fn kv_set(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    if value.len() > MAX_VALUE_BYTES {
        return Err(format!(
            "value for \"{key}\" is {} bytes; limit is {MAX_VALUE_BYTES}",
            value.len()
        ));
    }

    // Graft back anything the live module did not declare. Undeclared keys are
    // written through unchanged — visible in the shell as unprotected.
    let value = &match (
        active_version(conn, ROOT_MODULE).ok().flatten(),
        kv_get(conn, key).ok().flatten(),
    ) {
        (Some(version), Some(existing)) => {
            match (
                schema_for(conn, version).get(key),
                serde_json::from_str::<Value>(&existing),
                serde_json::from_str::<Value>(value),
            ) {
                (Some(decl), Ok(stored), Ok(incoming)) => {
                    merge_preserving_unknown(decl, &stored, incoming).to_string()
                }
                _ => value.to_string(),
            }
        }
        _ => value.to_string(),
    };

    conn.execute(
        "INSERT INTO kv (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, value, now_ms()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        ensure_seeded(&conn).unwrap();
        conn
    }

    #[test]
    fn seeds_once_and_is_live() {
        let c = fresh();
        ensure_seeded(&c).unwrap(); // idempotent
        assert_eq!(list_versions(&c, ROOT_MODULE).unwrap().len(), 1);
        assert_eq!(load_active(&c, ROOT_MODULE).unwrap().unwrap(), SEED_SOURCE);
    }

    #[test]
    fn save_appends_and_moves_the_pointer() {
        let c = fresh();
        let v = save(&c, ROOT_MODULE, "// v2", Some("edit")).unwrap();
        assert_eq!(v, 2);
        assert_eq!(load_active(&c, ROOT_MODULE).unwrap().unwrap(), "// v2");

        let rows = list_versions(&c, ROOT_MODULE).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].version, 2); // newest first
        assert!(rows[0].active);
        assert!(!rows[1].active);
    }

    #[test]
    fn rollback_moves_pointer_without_destroying_anything() {
        let c = fresh();
        save(&c, ROOT_MODULE, "// v2", None).unwrap();
        save(&c, ROOT_MODULE, "// v3", None).unwrap();

        set_active(&c, ROOT_MODULE, 1).unwrap();
        assert_eq!(load_active(&c, ROOT_MODULE).unwrap().unwrap(), SEED_SOURCE);

        // Every version survives the rollback — that is the whole point.
        assert_eq!(list_versions(&c, ROOT_MODULE).unwrap().len(), 3);
        assert_eq!(load_version(&c, ROOT_MODULE, 3).unwrap().unwrap(), "// v3");

        // And a later save still lands on top, not over v2.
        assert_eq!(save(&c, ROOT_MODULE, "// v4", None).unwrap(), 4);
    }

    #[test]
    fn save_records_its_parent() {
        let c = fresh();
        save(&c, ROOT_MODULE, "// v2", None).unwrap();
        set_active(&c, ROOT_MODULE, 1).unwrap();
        save(&c, ROOT_MODULE, "// v3 from v1", None).unwrap();

        let parent: Option<i64> = c
            .query_row(
                "SELECT parent_version FROM modules WHERE name=?1 AND version=3",
                [ROOT_MODULE],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(parent, Some(1), "v3 branched off the rolled-back v1");
    }

    #[test]
    fn rollback_to_unknown_version_is_refused() {
        let c = fresh();
        assert!(set_active(&c, ROOT_MODULE, 99).is_err());
        assert_eq!(load_active(&c, ROOT_MODULE).unwrap().unwrap(), SEED_SOURCE);
    }

    #[test]
    fn kv_roundtrips_and_is_capped() {
        let c = fresh();
        assert_eq!(kv_get(&c, "todos").unwrap(), None);
        kv_set(&c, "todos", "[1,2]").unwrap();
        assert_eq!(kv_get(&c, "todos").unwrap().unwrap(), "[1,2]");
        kv_set(&c, "todos", "[3]").unwrap();
        assert_eq!(kv_get(&c, "todos").unwrap().unwrap(), "[3]");

        let huge = "x".repeat(MAX_VALUE_BYTES + 1);
        assert!(kv_set(&c, "big", &huge).is_err());
        assert_eq!(kv_get(&c, "big").unwrap(), None);
    }
}

#[cfg(test)]
mod chat_tests {
    use super::*;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn
    }

    #[test]
    fn transcript_is_ordered_and_links_to_versions() {
        let c = fresh();
        let u = chat_append(&c, "user", "add due dates").unwrap();
        let a = chat_append(&c, "assistant", "Added a date field.").unwrap();
        chat_link_version(&c, a, 2).unwrap();

        let rows = chat_history(&c).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, u);
        assert_eq!(rows[0].role, "user");
        assert_eq!(rows[1].version, Some(2));
    }

    #[test]
    fn discarded_proposals_keep_their_turn_with_no_version() {
        let c = fresh();
        chat_append(&c, "user", "make it purple").unwrap();
        chat_append(&c, "assistant", "Recoloured everything.").unwrap();
        let rows = chat_history(&c).unwrap();
        assert_eq!(rows[1].version, None);
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        ensure_seeded(&conn).unwrap();
        conn
    }

    /// The scenario that motivated snapshots: a newer module adds a field, you
    /// switch back to an older one, and the older one writes back without it.
    #[test]
    fn an_older_module_cannot_permanently_strip_a_newer_field() {
        let c = fresh();
        kv_set(&c, "todos", r#"[{"id":1,"title":"ship","done":false}]"#).unwrap();

        // v2 introduces due dates and writes them.
        let v2 = save(&c, ROOT_MODULE, "// v2 with due dates", Some("add due dates")).unwrap();
        snapshot_kv(&c, Some(1), v2, "module change").unwrap();
        kv_set(
            &c,
            "todos",
            r#"[{"id":1,"title":"ship","done":false,"due":"2026-09-20"}]"#,
        )
        .unwrap();

        // Switch back to v1. The boundary is snapshotted before the pointer moves.
        let at_switch = snapshot_kv(&c, Some(v2), 1, "version switch").unwrap();
        set_active(&c, ROOT_MODULE, 1).unwrap();

        // v1 rebuilds its objects and drops the field it never knew about.
        kv_set(&c, "todos", r#"[{"id":1,"title":"ship","done":false}]"#).unwrap();
        assert!(!kv_get(&c, "todos").unwrap().unwrap().contains("due"));

        // The due date is still recoverable from the switch snapshot.
        let saved: String = c
            .query_row(
                "SELECT value FROM kv_snapshot_entries WHERE snapshot_id = ?1 AND key = 'todos'",
                [at_switch],
                |r| r.get(0),
            )
            .unwrap();
        assert!(saved.contains("2026-09-20"));

        let restored = restore_snapshot(&c, at_switch, 1).unwrap();
        assert_eq!(restored, 1);
        assert!(kv_get(&c, "todos").unwrap().unwrap().contains("2026-09-20"));
    }

    #[test]
    fn restoring_is_itself_undoable() {
        let c = fresh();
        kv_set(&c, "todos", "[1]").unwrap();
        let first = snapshot_kv(&c, None, 1, "module change").unwrap();
        kv_set(&c, "todos", "[2]").unwrap();

        restore_snapshot(&c, first, 1).unwrap();
        assert_eq!(kv_get(&c, "todos").unwrap().unwrap(), "[1]");

        // The pre-restore state was captured on the way past, so "[2]" is not lost.
        let snaps = list_snapshots(&c).unwrap();
        let pre = snaps.iter().find(|s| s.reason == "before restore").unwrap();
        let held: String = c
            .query_row(
                "SELECT value FROM kv_snapshot_entries WHERE snapshot_id = ?1 AND key = 'todos'",
                [pre.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(held, "[2]");
    }

    #[test]
    fn snapshots_are_bounded_by_switches_not_by_writes() {
        let c = fresh();
        for i in 0..500 {
            kv_set(&c, "todos", &format!("[{i}]")).unwrap();
        }
        assert_eq!(list_snapshots(&c).unwrap().len(), 0, "writes alone snapshot nothing");
        snapshot_kv(&c, Some(1), 2, "module change").unwrap();
        assert_eq!(list_snapshots(&c).unwrap().len(), 1);
    }

    #[test]
    fn settings_round_trip_with_a_fallback() {
        let c = fresh();
        assert_eq!(get_setting(&c, "model", "claude-sonnet-5"), "claude-sonnet-5");
        set_setting(&c, "model", "claude-opus-5").unwrap();
        assert_eq!(get_setting(&c, "model", "claude-sonnet-5"), "claude-opus-5");
    }
}

#[cfg(test)]
mod schema_tests {
    use super::*;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        ensure_seeded(&conn).unwrap();
        conn
    }

    fn todo_list() -> Declaration {
        Declaration::RecordList {
            key: "id".into(),
            fields: vec!["id".into(), "title".into(), "done".into()],
        }
    }

    /// The M5 scenario, now prevented rather than merely recoverable.
    #[test]
    fn an_older_module_cannot_strip_a_field_it_never_declared() {
        let c = fresh();
        let mut schema = Schema::new();
        schema.insert("todos".into(), todo_list());
        declare_schema(&c, 1, &schema).unwrap();

        kv_set(
            &c,
            "todos",
            r#"[{"id":1,"title":"ship","done":false,"due":"2026-09-20"}]"#,
        )
        .unwrap();

        // An older module rebuilds its objects and omits `due` entirely.
        kv_set(&c, "todos", r#"[{"id":1,"title":"ship","done":true}]"#).unwrap();

        let after = kv_get(&c, "todos").unwrap().unwrap();
        assert!(after.contains("2026-09-20"), "undeclared field must survive: {after}");
        assert!(after.contains(r#""done":true"#), "declared field must update: {after}");
    }

    #[test]
    fn a_declared_field_can_be_removed_by_its_owner() {
        let c = fresh();
        let mut schema = Schema::new();
        schema.insert("todos".into(), todo_list());
        declare_schema(&c, 1, &schema).unwrap();

        kv_set(&c, "todos", r#"[{"id":1,"title":"ship","done":true}]"#).unwrap();
        // `done` is declared, so dropping it is an intentional change, not a loss.
        kv_set(&c, "todos", r#"[{"id":1,"title":"ship"}]"#).unwrap();
        assert!(!kv_get(&c, "todos").unwrap().unwrap().contains("done"));
    }

    #[test]
    fn deleting_a_row_actually_deletes_it() {
        let c = fresh();
        let mut schema = Schema::new();
        schema.insert("todos".into(), todo_list());
        declare_schema(&c, 1, &schema).unwrap();

        kv_set(
            &c,
            "todos",
            r#"[{"id":1,"title":"a","due":"x"},{"id":2,"title":"b"}]"#,
        )
        .unwrap();
        kv_set(&c, "todos", r#"[{"id":2,"title":"b"}]"#).unwrap();

        let after = kv_get(&c, "todos").unwrap().unwrap();
        assert!(!after.contains("\"id\":1"), "a dropped row stays dropped: {after}");
        assert!(after.contains("\"id\":2"));
    }

    #[test]
    fn an_undeclared_key_is_written_through_unprotected() {
        let c = fresh();
        declare_schema(&c, 1, &Schema::new()).unwrap();
        kv_set(&c, "notes", r#"{"a":1,"b":2}"#).unwrap();
        kv_set(&c, "notes", r#"{"a":9}"#).unwrap();
        assert_eq!(kv_get(&c, "notes").unwrap().unwrap(), r#"{"a":9}"#);
    }

    #[test]
    fn a_new_row_needs_no_history() {
        let c = fresh();
        let mut schema = Schema::new();
        schema.insert("todos".into(), todo_list());
        declare_schema(&c, 1, &schema).unwrap();
        kv_set(&c, "todos", r#"[{"id":1,"title":"a"}]"#).unwrap();
        kv_set(&c, "todos", r#"[{"id":1,"title":"a"},{"id":2,"title":"new"}]"#).unwrap();
        let after = kv_get(&c, "todos").unwrap().unwrap();
        assert!(after.contains("new"));
    }

    #[test]
    fn schema_survives_a_round_trip_through_json() {
        let c = fresh();
        let mut schema = Schema::new();
        schema.insert("todos".into(), todo_list());
        schema.insert("prefs".into(), Declaration::Record { fields: vec!["theme".into()] });
        schema.insert("counter".into(), Declaration::Scalar);
        declare_schema(&c, 7, &schema).unwrap();

        let back = schema_for(&c, 7);
        assert_eq!(back.len(), 3);
        assert!(matches!(back.get("counter"), Some(Declaration::Scalar)));
        assert!(matches!(back.get("todos"), Some(Declaration::RecordList { .. })));
    }
}

#[cfg(test)]
mod integrity_tests {
    use super::*;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        ensure_seeded(&conn).unwrap();
        conn
    }

    #[test]
    fn a_clean_history_verifies_and_has_a_head() {
        let c = fresh();
        save(&c, ROOT_MODULE, "// v2", Some("two")).unwrap();
        save(&c, ROOT_MODULE, "// v3", Some("three")).unwrap();

        let report = verify_integrity(&c, ROOT_MODULE).unwrap();
        assert!(report.ok);
        assert_eq!(report.versions, 3);
        assert_eq!(report.broken_at, None);
        assert_eq!(report.head.as_ref().unwrap().len(), 64);
    }

    /// The scenario the chain exists for: someone edits the SQLite file directly.
    #[test]
    fn editing_a_stored_source_is_detected() {
        let c = fresh();
        save(&c, ROOT_MODULE, "// v2", Some("two")).unwrap();
        save(&c, ROOT_MODULE, "// v3", Some("three")).unwrap();
        assert!(verify_integrity(&c, ROOT_MODULE).unwrap().ok);

        c.execute(
            "UPDATE modules SET source = '// tampered' WHERE version = 2",
            [],
        )
        .unwrap();

        let report = verify_integrity(&c, ROOT_MODULE).unwrap();
        assert!(!report.ok);
        assert_eq!(report.broken_at, Some(2), "flags the earliest altered row");
    }

    #[test]
    fn editing_a_note_is_detected_too() {
        let c = fresh();
        save(&c, ROOT_MODULE, "// v2", Some("two")).unwrap();
        c.execute("UPDATE modules SET note = 'innocent' WHERE version = 2", [])
            .unwrap();
        assert_eq!(verify_integrity(&c, ROOT_MODULE).unwrap().broken_at, Some(2));
    }

    #[test]
    fn deleting_history_is_detected() {
        let c = fresh();
        save(&c, ROOT_MODULE, "// v2", Some("two")).unwrap();
        save(&c, ROOT_MODULE, "// v3", Some("three")).unwrap();
        let before = verify_integrity(&c, ROOT_MODULE).unwrap().head.unwrap();

        c.execute("DELETE FROM modules WHERE version = 2", []).unwrap();
        let after = verify_integrity(&c, ROOT_MODULE).unwrap();

        assert!(!after.ok);
        assert_ne!(after.head.unwrap(), before, "the head must move");
    }

    /// Tampering early has to invalidate everything after it, or the chain is
    /// just a per-row checksum and an attacker can re-stamp one row.
    #[test]
    fn a_tampered_row_cannot_be_rehashed_in_isolation() {
        let c = fresh();
        save(&c, ROOT_MODULE, "// v2", Some("two")).unwrap();
        save(&c, ROOT_MODULE, "// v3", Some("three")).unwrap();

        // Edit v2 AND re-stamp its hash, as someone covering their tracks would.
        let forged = hash_version(
            Some(&hash_version(None, ROOT_MODULE, 1, SEED_SOURCE, Some("seed"))),
            ROOT_MODULE,
            2,
            "// tampered",
            Some("two"),
        );
        c.execute(
            "UPDATE modules SET source = '// tampered', hash = ?1 WHERE version = 2",
            [&forged],
        )
        .unwrap();

        // v2 now self-consistently verifies, but v3's stored hash folded in the
        // ORIGINAL v2, so the break simply moves downstream.
        let report = verify_integrity(&c, ROOT_MODULE).unwrap();
        assert!(!report.ok);
        assert_eq!(report.broken_at, Some(3));
    }

    #[test]
    fn backfill_stamps_legacy_rows_without_changing_the_chain() {
        let c = fresh();
        save(&c, ROOT_MODULE, "// v2", Some("two")).unwrap();
        let head = verify_integrity(&c, ROOT_MODULE).unwrap().head.unwrap();

        c.execute("UPDATE modules SET hash = NULL", []).unwrap();
        assert!(!verify_integrity(&c, ROOT_MODULE).unwrap().ok);

        assert_eq!(backfill_hashes(&c, ROOT_MODULE).unwrap(), 2);
        let after = verify_integrity(&c, ROOT_MODULE).unwrap();
        assert!(after.ok);
        assert_eq!(after.head.unwrap(), head, "backfill reproduces the same head");
    }
}

#[cfg(test)]
mod reset_tests {
    use super::*;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        ensure_seeded(&conn).unwrap();
        conn
    }

    /// The behaviour that motivated this: after resetting to v1, the next edit
    /// must be v2 — not v11 with ten abandoned versions still in the list.
    #[test]
    fn the_next_version_after_a_reset_is_the_next_number() {
        let c = fresh();
        for i in 2..=10 {
            save(&c, ROOT_MODULE, &format!("// v{i}"), None).unwrap();
        }
        assert_eq!(list_versions(&c, ROOT_MODULE).unwrap().len(), 10);

        let out = reset_to(&c, ROOT_MODULE, 1, None).unwrap();
        assert_eq!(out.removed.len(), 9);
        assert_eq!(out.active, 1);
        assert_eq!(list_versions(&c, ROOT_MODULE).unwrap().len(), 1);

        assert_eq!(save(&c, ROOT_MODULE, "// fresh v2", None).unwrap(), 2);
    }

    #[test]
    fn reset_clears_app_data() {
        let c = fresh();
        kv_set(&c, "todos", r#"[{"id":1}]"#).unwrap();
        save(&c, ROOT_MODULE, "// v2", None).unwrap();

        let out = reset_to(&c, ROOT_MODULE, 1, None).unwrap();
        assert!(out.cleared_keys > 0);
        assert_eq!(kv_get(&c, "todos").unwrap(), None);
    }

    /// Destructive, but not a cliff — the data is recoverable from the snapshot.
    #[test]
    fn reset_snapshots_the_data_it_is_about_to_clear() {
        let c = fresh();
        kv_set(&c, "todos", r#"[{"id":1,"title":"keep me"}]"#).unwrap();
        save(&c, ROOT_MODULE, "// v2", None).unwrap();

        let out = reset_to(&c, ROOT_MODULE, 1, None).unwrap();
        let held: String = c
            .query_row(
                "SELECT value FROM kv_snapshot_entries WHERE snapshot_id = ?1 AND key = 'todos'",
                [out.snapshot_id],
                |r| r.get(0),
            )
            .unwrap();
        assert!(held.contains("keep me"));

        restore_snapshot(&c, out.snapshot_id, 1).unwrap();
        assert!(kv_get(&c, "todos").unwrap().unwrap().contains("keep me"));
    }

    #[test]
    fn the_chain_is_valid_after_a_reset() {
        let c = fresh();
        for i in 2..=5 {
            save(&c, ROOT_MODULE, &format!("// v{i}"), None).unwrap();
        }
        let before = verify_integrity(&c, ROOT_MODULE).unwrap().head.unwrap();
        let out = reset_to(&c, ROOT_MODULE, 2, None).unwrap();

        let after = verify_integrity(&c, ROOT_MODULE).unwrap();
        assert!(after.ok, "survivors must not read as tampered");
        // The head moves because HEAD moved — it is now v2's own hash, unchanged
        // from before the reset. Nothing was rewritten. See truncation_tests.
        assert_ne!(out.head.unwrap(), before);
    }

    #[test]
    fn resetting_to_the_live_version_just_clears_data() {
        let c = fresh();
        kv_set(&c, "todos", "[1]").unwrap();
        let out = reset_to(&c, ROOT_MODULE, 1, None).unwrap();
        assert!(out.removed.is_empty());
        assert_eq!(kv_get(&c, "todos").unwrap(), None);
        assert_eq!(list_versions(&c, ROOT_MODULE).unwrap().len(), 1);
    }

    #[test]
    fn resetting_to_an_unknown_version_changes_nothing() {
        let c = fresh();
        save(&c, ROOT_MODULE, "// v2", None).unwrap();
        assert!(reset_to(&c, ROOT_MODULE, 99, None).is_err());
        assert_eq!(list_versions(&c, ROOT_MODULE).unwrap().len(), 2);
    }
}

#[cfg(test)]
mod truncation_tests {
    use super::*;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        ensure_seeded(&conn).unwrap();
        conn
    }

    fn hashes(c: &Connection) -> Vec<(i64, String)> {
        let mut stmt = c
            .prepare("SELECT version, hash FROM modules WHERE name=?1 ORDER BY version")
            .unwrap();
        stmt.query_map([ROOT_MODULE], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// A truncation is a suffix removal, so no surviving hash can depend on a row
    /// that went. Same property as `git reset --hard`: the commits that remain
    /// keep their SHAs. Anyone who recorded a hash before the reset can still
    /// verify it afterwards.
    #[test]
    fn surviving_hashes_are_byte_identical_after_a_reset() {
        let c = fresh();
        for i in 2..=8 {
            save(&c, ROOT_MODULE, &format!("// v{i}"), Some("edit")).unwrap();
        }
        let before: Vec<_> = hashes(&c).into_iter().filter(|(v, _)| *v <= 3).collect();

        reset_to(&c, ROOT_MODULE, 3, None).unwrap();

        assert_eq!(hashes(&c), before, "truncation must not disturb what remains");
        assert!(verify_integrity(&c, ROOT_MODULE).unwrap().ok);
    }

    /// The head moves because HEAD moved, not because anything was rewritten.
    #[test]
    fn the_head_becomes_the_surviving_tip_not_a_new_value() {
        let c = fresh();
        for i in 2..=5 {
            save(&c, ROOT_MODULE, &format!("// v{i}"), None).unwrap();
        }
        let tip_of_v2 = hashes(&c).into_iter().find(|(v, _)| *v == 2).unwrap().1;

        let out = reset_to(&c, ROOT_MODULE, 2, None).unwrap();
        assert_eq!(out.head.unwrap(), tip_of_v2, "head is v2's original hash");
    }

}

#[cfg(test)]
mod immutability_tests {
    use super::*;
    use std::collections::HashMap;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        ensure_seeded(&conn).unwrap();
        conn
    }

    fn hashes(c: &Connection) -> HashMap<i64, String> {
        let mut stmt = c
            .prepare("SELECT version, hash FROM modules WHERE name=?1")
            .unwrap();
        stmt.query_map([ROOT_MODULE], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    /// The guarantee that removing middle-deletion buys: **no operation this app
    /// offers ever overwrites a hash it has already written.**
    ///
    /// Every mutation runs here. A version's hash is written once, when the
    /// version is created, and is either still that value or the row is gone.
    /// So a hash recorded at any point in the past still verifies today, and
    /// "the history has not been edited" is unconditional rather than
    /// "…since the last compaction".
    #[test]
    fn no_operation_ever_rewrites_an_existing_hash() {
        let c = fresh();
        for i in 2..=6 {
            save(&c, ROOT_MODULE, &format!("// v{i}"), Some("edit")).unwrap();
        }
        let mut schema = Schema::new();
        schema.insert(
            "todos".into(),
            Declaration::RecordList { key: "id".into(), fields: vec!["id".into()] },
        );
        declare_schema(&c, 6, &schema).unwrap();
        let original = hashes(&c);
        assert_eq!(original.len(), 6);

        // Every mutating path in the app, in turn.
        kv_set(&c, "todos", r#"[{"id":1}]"#).unwrap();
        set_active(&c, ROOT_MODULE, 3).unwrap();
        let snap = snapshot_kv(&c, Some(3), 4, "version switch").unwrap();
        set_active(&c, ROOT_MODULE, 4).unwrap();
        restore_snapshot(&c, snap, 4).unwrap();
        save(&c, ROOT_MODULE, "// v7", Some("more")).unwrap();
        mark_failed(&c, ROOT_MODULE, 7).unwrap();
        chat_append(&c, "user", "hello").unwrap();
        set_setting(&c, "model", "claude-opus-5").unwrap();

        for (version, hash) in &original {
            assert_eq!(
                hashes(&c).get(version),
                Some(hash),
                "v{version} was rewritten"
            );
        }

        // And after the one destructive operation, survivors are still untouched.
        reset_to(&c, ROOT_MODULE, 3, None).unwrap();
        let after = hashes(&c);
        assert_eq!(after.len(), 3);
        for (version, hash) in &original {
            if let Some(now) = after.get(version) {
                assert_eq!(now, hash, "v{version} was rewritten by the reset");
            }
        }
        assert!(verify_integrity(&c, ROOT_MODULE).unwrap().ok);
    }
}

#[cfg(test)]
mod stale_reset_tests {
    use super::*;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        ensure_seeded(&conn).unwrap();
        conn
    }

    fn head8(c: &Connection) -> String {
        verify_integrity(c, ROOT_MODULE).unwrap().head.unwrap()[..8].to_string()
    }

    #[test]
    fn a_reset_agreeing_with_what_was_on_screen_proceeds() {
        let c = fresh();
        save(&c, ROOT_MODULE, "// v2", None).unwrap();
        let seen = head8(&c);
        let out = reset_to(&c, ROOT_MODULE, 1, Some(&seen)).unwrap();
        assert_eq!(out.removed, vec![2]);
    }

    /// What actually happened: a version landed between the shell's last refresh
    /// and the click, so the confirm said "delete 2" while the truth was 3.
    #[test]
    fn a_reset_on_a_stale_view_is_refused_and_deletes_nothing() {
        let c = fresh();
        save(&c, ROOT_MODULE, "// v2", None).unwrap();
        let seen = head8(&c);

        // A generation lands after the user read the screen.
        save(&c, ROOT_MODULE, "// v3 arrived late", None).unwrap();

        let err = reset_to(&c, ROOT_MODULE, 1, Some(&seen)).unwrap_err();
        assert!(err.contains("history moved"), "{err}");
        assert_eq!(list_versions(&c, ROOT_MODULE).unwrap().len(), 3, "nothing deleted");
        assert!(verify_integrity(&c, ROOT_MODULE).unwrap().ok);
    }

    #[test]
    fn a_scripted_reset_passes_no_head_and_is_not_blocked() {
        let c = fresh();
        save(&c, ROOT_MODULE, "// v2", None).unwrap();
        save(&c, ROOT_MODULE, "// v3", None).unwrap();
        assert!(reset_to(&c, ROOT_MODULE, 1, None).is_ok());
    }
}
