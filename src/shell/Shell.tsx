import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";

const MODULE = "app";
const MODELS = [
  { id: "claude-sonnet-5", label: "Sonnet 5", cost: "$2 / $10 per Mtok" },
  { id: "claude-opus-5", label: "Opus 5", cost: "$5 / $25 per Mtok" },
];

interface ProbeReport {
  ok: boolean;
  isolated: boolean | null;
  phase?: string;
  error?: string;
  mounted: boolean;
  nodeCount: number;
}
interface VersionRow {
  version: number;
  note: string | null;
  created_at: number;
  bytes: number;
  active: boolean;
  failed: boolean;
}
interface ChatRow {
  id: number;
  role: string;
  content: string;
  version: number | null;
  created_at: number;
}
interface SnapshotRow {
  id: number;
  from_version: number | null;
  to_version: number;
  reason: string;
  taken_at: number;
  keys: number;
}
interface CanvasTheme {
  accentColor: string;
  grayColor: string;
  radius: string;
  scaling: string;
  appearance: string;
}

const ACCENTS = ["indigo", "blue", "cyan", "teal", "jade", "green", "grass", "amber",
  "orange", "tomato", "red", "ruby", "crimson", "pink", "plum", "purple", "violet",
  "iris", "brown", "bronze", "gold", "gray"];
const GRAYS = ["auto", "gray", "mauve", "slate", "sage", "olive", "sand"];
const RADII = ["none", "small", "medium", "large", "full"];
const APPEARANCES = ["inherit", "light", "dark"];
const SCALINGS = ["90%", "95%", "100%", "105%", "110%"];

interface TemplateInfo {
  id: string;
  name: string;
  description: string;
  bytes: number;
}
interface BuildIdentity {
  dev: boolean;
  engine: string;
  app: string | null;
  ok: boolean;
  versions: number;
  broken_at: number | null;
}
interface UpdateStatus {
  available: boolean;
  version: string | null;
  notes: string | null;
  error: string | null;
}
interface KeyStatus {
  key: string;
  protected: boolean;
  shape: string | null;
}
interface Proposal {
  chat_id: number;
  source: string;
  note: string;
  explanation: string;
}
interface ApplyOutcome {
  applied: boolean;
  version: number;
  rolled_back_to: number | null;
  phase: string | null;
  error: string | null;
  snapshot_id: number;
}

export function Shell() {
  const [probe, setProbe] = useState<ProbeReport | null>(null);
  const [versions, setVersions] = useState<VersionRow[]>([]);
  const [chat, setChat] = useState<ChatRow[]>([]);
  const [snapshots, setSnapshots] = useState<SnapshotRow[]>([]);
  const [keys, setKeys] = useState<KeyStatus[]>([]);
  const [build, setBuild] = useState<BuildIdentity | null>(null);
  const [update, setUpdate] = useState<UpdateStatus | null>(null);
  const [templates, setTemplates] = useState<TemplateInfo[]>([]);
  const [theme, setTheme] = useState<CanvasTheme | null>(null);
  const [collapsed, setCollapsed] = useState(false);
  const [hideFailed, setHideFailed] = useState(false);
  const [confirmReset, setConfirmReset] = useState<number | null>(null);
  /** Shown while the model is thinking. The real row does not exist until Rust
   *  writes it after the API call returns, so without this the message you just
   *  sent is nowhere: cleared from the input and not yet in the transcript. */
  const [pending, setPending] = useState<string | null>(null);
  const [source, setSource] = useState("");
  const [model, setModel] = useState("claude-sonnet-5");
  const [keySource, setKeySource] = useState<string | null>(null);
  const [keyDraft, setKeyDraft] = useState("");
  const [prompt, setPrompt] = useState("");
  const [flash, setFlash] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const tail = useRef<HTMLDivElement>(null);

  const refresh = useCallback(async () => {
    setVersions(await invoke<VersionRow[]>("list_versions", { name: MODULE }));
    setChat(await invoke<ChatRow[]>("chat_history"));
    setSnapshots(await invoke<SnapshotRow[]>("list_snapshots"));
    setKeys(await invoke<KeyStatus[]>("schema_status"));
    setBuild(await invoke<BuildIdentity>("build_identity"));
    setTemplates(await invoke<TemplateInfo[]>("list_templates"));
    setTheme(await invoke<CanvasTheme>("get_theme"));
    setSource(await invoke<string>("load_active_module", { name: MODULE }));
    setProbe(await invoke<ProbeReport>("probe_canvas"));
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        setKeySource(await invoke<string>("api_key_status"));
        setModel(await invoke<string>("get_model"));
        await refresh();
      } catch (e) {
        setErr(String(e));
      }
    })();
  }, [refresh]);

  useEffect(() => {
    tail.current?.scrollIntoView({ behavior: "smooth" });
  }, [chat.length, busy]);

  // Rust resizes the frame; we fade the content just before it narrows, so text
  // does not visibly reflow as it is clipped.
  useEffect(() => {
    const a = listen("drawer:collapse", () => setCollapsed(true));
    const b = listen("drawer:expand", () => setCollapsed(false));
    return () => {
      void a.then((f) => f());
      void b.then((f) => f());
    };
  }, []);

  const run = async (label: string, fn: () => Promise<void>) => {
    setBusy(label);
    setErr(null);
    try {
      await fn();
    } catch (e) {
      setErr(String(e));
      // A refused reset means the panel is stale — show the truth immediately.
      if (String(e).includes("history moved")) {
        setConfirmReset(null);
        void refresh();
      }
      if (pending) {
        // Hand the prompt back rather than losing it to a failed call.
        setPrompt((current) => current || pending);
        setPending(null);
      }
    } finally {
      setBusy(null);
    }
  };

  const report = (o: ApplyOutcome) =>
    setFlash(
      o.applied
        ? `v${o.version} is live.`
        : `v${o.version} failed at ${o.phase ?? "startup"} — switched back to v${o.rolled_back_to}. It is kept in the history so you can read it.`
    );

  const send = () =>
    run("Thinking", async () => {
      const text = prompt.trim();
      if (!text) return;
      setPrompt("");
      setPending(text);
      setFlash(null);
      const p = await invoke<Proposal>("propose_change", { prompt: text });
      setBusy("Verifying");
      const outcome = await invoke<ApplyOutcome>("apply_and_verify", {
        name: MODULE,
        source: p.source,
        note: p.note,
      });
      if (outcome.applied) {
        await invoke("link_chat_version", { id: p.chat_id, version: outcome.version });
      }
      report(outcome);
      await refresh();
    });

  /** Applies to every version ever saved — the theme lives outside the module,
   *  so nothing is regenerated and no model call is made. */
  const changeTheme = (patch: Partial<CanvasTheme>) =>
    run("Theming", async () => {
      if (!theme) return;
      const next = { ...theme, ...patch };
      setTheme(next);
      await invoke("set_theme", { theme: next });
    });

  const startFrom = (id: string, name: string) =>
    run(`Loading ${name}`, async () => {
      setFlash(null);
      const outcome = await invoke<ApplyOutcome>("apply_template", { id });
      report(outcome);
      await refresh();
    });

  const switchTo = (version: number) =>
    run(`Switching to v${version}`, async () => {
      setFlash(null);
      await invoke("set_active_version", { name: MODULE, version });
      await invoke("reload_canvas");
      await new Promise((r) => setTimeout(r, 600));
      setFlash(`Switched to v${version}. Data snapshotted at the boundary.`);
      await refresh();
    });

  /** Destructive: throws away every version after this one and clears app data.
   *  The data is snapshotted first, so a mistaken reset is recoverable. */
  const resetTo = (version: number) =>
    run(`Resetting to v${version}`, async () => {
      setFlash(null);
      // Send the head the panel was showing. Rust refuses if the history moved
      // since — the confirm promised to delete N versions, and it must not
      // quietly delete more than that.
      const out = await invoke<{
        removed: number[];
        active: number;
        cleared_keys: number;
        head: string | null;
      }>("reset_to_version", {
        name: MODULE,
        version,
        expectedHead: build?.app ?? null,
      });
      setConfirmReset(null);
      setFlash(
        `Reset to v${out.active}. ${out.removed.length} version${
          out.removed.length === 1 ? "" : "s"
        } removed, data cleared — recoverable from the newest snapshot. Next edit will be v${
          out.active + 1
        }.`
      );
      await refresh();
    });

  const restore = (id: number) =>
    run(`Restoring data`, async () => {
      const keys = await invoke<number>("restore_snapshot", { id });
      await invoke("reload_canvas");
      await new Promise((r) => setTimeout(r, 600));
      setFlash(`Restored ${keys} key${keys === 1 ? "" : "s"} from snapshot #${id}.`);
      await refresh();
    });

  const checkUpdate = () =>
    run("Checking", async () => {
      setUpdate(await invoke<UpdateStatus>("check_for_update"));
    });

  /** Explicit, never automatic: an engine update is the one change in this app
   *  that a version switch cannot undo. */
  const applyUpdate = () =>
    run("Updating", async () => {
      await invoke("install_update");
    });

  const changeModel = (id: string) =>
    run("Switching model", async () => {
      await invoke("set_model", { model: id });
      setModel(id);
    });

  const saveKey = () =>
    run("Saving key", async () => {
      await invoke("set_api_key", { key: keyDraft });
      setKeyDraft("");
      setKeySource(await invoke<string>("api_key_status"));
    });

  const healthy = probe?.ok === true;
  const keySet = keySource !== null && keySource !== "none";

  const unprotected = keys.filter((k) => !k.protected);
  const failedCount = versions.filter((v) => v.failed).length;
  const shownVersions = hideFailed ? versions.filter((v) => !v.failed) : versions;
  // A install nobody has done anything with yet: one version, nothing said.
  const fresh = versions.length <= 1 && chat.length === 0;

  // Collapsed, the drawer is a rail rather than nothing. The chevron always
  // points the way the panel will move.
  if (collapsed) {
    return (
      <div className="rail">
        <button
          className="collapse"
          title="Expand chat"
          onClick={() => void emit("drawer:request-close")}
        >
          ›
        </button>
        <span className={`dot ${healthy ? "good" : "bad"}`} title={healthy ? "canvas healthy" : "canvas unhealthy"} />
      </div>
    );
  }

  return (
    <div className="shell">
      <header>
        <div>
          <h1>Chat</h1>
          <p>Privileged window. Never model-authored.</p>
        </div>
        <button
          className="collapse"
          title="Collapse to a rail"
          onClick={() => void emit("drawer:request-close")}
        >
          ‹
        </button>
      </header>

      <section className="status">
        <div className="chips">
          <span className={`chip ${healthy ? "good" : "bad"}`} title="canvas health">
            {probe === null ? "…" : healthy ? "healthy" : "unhealthy"}
          </span>
          <span
            className={`chip ${probe?.isolated ? "good" : "bad"}`}
            title="capability isolation — the canvas must not reach a privileged command"
          >
            {probe == null || probe.isolated === null
              ? "…"
              : probe.isolated
                ? "isolated"
                : "BREACHED"}
          </span>
          <span
            className={`chip ${build && !build.ok ? "bad" : ""}`}
            title="engine version · head of the module-history hash chain"
          >
            {build ? `${build.engine} · ${build.app ?? "—"}` : "…"}
          </span>
        </div>

        <div className="controls">
          <select value={model} onChange={(e) => changeModel(e.target.value)} disabled={!!busy}>
            {MODELS.map((m) => (
              <option key={m.id} value={m.id}>
                {m.label} · {m.cost}
              </option>
            ))}
          </select>
          {update === null ? (
            <button className="inline" onClick={checkUpdate} disabled={!!busy}>
              Check engine
            </button>
          ) : update.available ? (
            <button className="inline good" onClick={applyUpdate} disabled={!!busy}>
              Install {update.version}
            </button>
          ) : (
            <span className="chip dim" title={update.error ?? undefined}>
              {update.error ? "engine: check failed" : "engine: up to date"}
            </span>
          )}
        </div>

        {build && !build.ok && (
          <pre className="err">
            History integrity failed at v{build.broken_at} — a stored version was
            altered outside the app.
          </pre>
        )}
        {probe?.error && (
          <pre className="err">
            {probe.phase}: {probe.error}
          </pre>
        )}
        {update?.error && <pre className="err">{update.error}</pre>}
        {flash && <p className="flash">{flash}</p>}
        {err && <pre className="err">{err}</pre>}
      </section>

      {keySource === "none" && (
        <section className="keygate">
          <label>Anthropic API key</label>
          <p className="note">
            Stored in the macOS Keychain, read only by Rust. In development,
            <code>export ANTHROPIC_API_KEY=…</code> avoids the Keychain prompt that
            every rebuild otherwise triggers.
          </p>
          <div className="actions">
            <input
              type="password"
              value={keyDraft}
              placeholder="sk-ant-…"
              onChange={(e) => setKeyDraft(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && saveKey()}
            />
            <button onClick={saveKey} disabled={!!busy || !keyDraft.trim()}>
              Save
            </button>
          </div>
        </section>
      )}

      {fresh && templates.length > 0 && (
        <section className="picker">
          <label>Start from a template</label>
          <div className="grid">
            {templates.map((t) => (
              <button
                key={t.id}
                className="tpl"
                disabled={!!busy}
                onClick={() => startFrom(t.id, t.name)}
              >
                <b>{t.name}</b>
                <span>{t.description}</span>
              </button>
            ))}
          </div>
          <p className="note">
            Or just describe what you want below. Either way the app keeps every
            version, so nothing here is a commitment.
          </p>
        </section>
      )}

      <section className="chat">
        <div className="transcript">
          {chat.length === 0 && (
            <p className="empty">
              Describe a change and the app rewrites itself. A version that fails to
              compile or mount is switched away from automatically.
            </p>
          )}
          {chat.map((m) => (
            <div key={m.id} className={`msg ${m.role}`}>
              <span>{m.content}</span>
              {m.version && <em className="badge">v{m.version}</em>}
            </div>
          ))}
          {pending && <div className="msg user">{pending}</div>}
          {busy && <div className="msg assistant pending">{busy}…</div>}
          <div ref={tail} />
        </div>

        <div className="composer">
          <textarea
            value={prompt}
            placeholder={keySet ? "How should the app change?" : "Add an API key first"}
            disabled={!keySet || !!busy}
            onChange={(e) => setPrompt(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.metaKey || !e.shiftKey)) {
                e.preventDefault();
                void send();
              }
            }}
          />
          <button onClick={send} disabled={!keySet || !!busy || !prompt.trim()}>
            Send
          </button>
        </div>
      </section>

      <div className="panels">
        <details className="versions">
          <summary>
            Versions · v{versions.find((v) => v.active)?.version ?? "—"} live ·{" "}
            {versions.length} total
          </summary>
          {failedCount > 0 && (
            <div className="panel-actions">
              <label>
                <input
                  type="checkbox"
                  checked={hideFailed}
                  onChange={(e) => setHideFailed(e.target.checked)}
                />
                Hide {failedCount} failed
              </label>
            </div>
          )}
          <ul>
            {shownVersions.map((v) => (
              <li
                key={v.version}
                className={`${v.active ? "active" : ""} ${v.failed ? "failed" : ""}`}
              >
                <span className="v">v{v.version}</span>
                <span className="note-col">
                  {v.failed && <span className="x" title="Rejected by the gate">✕ </span>}
                  {v.note ?? "—"}
                </span>
                <span className="row-actions">
                  {v.active ? (
                    <span className="badge">live</span>
                  ) : (
                    <button onClick={() => switchTo(v.version)} disabled={!!busy}>
                      Switch to
                    </button>
                  )}
                  {confirmReset === v.version ? (
                    <>
                      <button className="danger" onClick={() => resetTo(v.version)} disabled={!!busy}>
                        Delete {versions.filter((x) => x.version > v.version).length} + data
                      </button>
                      <button onClick={() => setConfirmReset(null)} disabled={!!busy}>
                        Cancel
                      </button>
                    </>
                  ) : (
                    <button
                      onClick={() => setConfirmReset(v.version)}
                      disabled={!!busy}
                      title="Truncate history to here and clear app data"
                    >
                      Reset to
                    </button>
                  )}
                </span>
              </li>
            ))}
          </ul>
        </details>

        <details className="templates">
          <summary>Templates · {templates.length}</summary>
          <p className="note">
            Loads as a new version, so your current one stays in the history. Your
            data is kept — sample rows are only seeded into an empty store.
          </p>
          <ul>
            {templates.map((t) => (
              <li key={t.id}>
                <span className="note-col">
                  <b>{t.name}</b> · {t.description}
                </span>
                <button onClick={() => startFrom(t.id, t.name)} disabled={!!busy}>
                  Start
                </button>
              </li>
            ))}
          </ul>
        </details>

        <details className="appearance">
          <summary>
            Appearance · {theme?.accentColor ?? "…"} · {theme?.radius ?? "…"}
          </summary>
          <p className="note">
            The theme wraps the module rather than living inside it, so a change here
            re-skins every version in your history at once — including ones written
            before the theme existed. No regeneration, no model call.
          </p>
          {theme && (
            <div className="theme-grid">
              <label>
                Accent
                <select
                  value={theme.accentColor}
                  onChange={(e) => changeTheme({ accentColor: e.target.value })}
                >
                  {ACCENTS.map((c) => (
                    <option key={c}>{c}</option>
                  ))}
                </select>
              </label>
              <label>
                Gray
                <select
                  value={theme.grayColor}
                  onChange={(e) => changeTheme({ grayColor: e.target.value })}
                >
                  {GRAYS.map((c) => (
                    <option key={c}>{c}</option>
                  ))}
                </select>
              </label>
              <label>
                Radius
                <select
                  value={theme.radius}
                  onChange={(e) => changeTheme({ radius: e.target.value })}
                >
                  {RADII.map((c) => (
                    <option key={c}>{c}</option>
                  ))}
                </select>
              </label>
              <label>
                Appearance
                <select
                  value={theme.appearance}
                  onChange={(e) => changeTheme({ appearance: e.target.value })}
                >
                  {APPEARANCES.map((c) => (
                    <option key={c}>{c}</option>
                  ))}
                </select>
              </label>
              <label>
                Scale
                <select
                  value={theme.scaling}
                  onChange={(e) => changeTheme({ scaling: e.target.value })}
                >
                  {SCALINGS.map((c) => (
                    <option key={c}>{c}</option>
                  ))}
                </select>
              </label>
            </div>
          )}
        </details>

        <details className="snapshots">
          <summary>Data snapshots · {snapshots.length}</summary>
          <p className="note">
            Taken at every version change. App data is shared across versions so new
            fields carry forward — these are the undo for when an older module writes
            back without a field it never knew about.
          </p>
          <ul>
            {snapshots.map((s) => (
              <li key={s.id}>
                <span className="v">#{s.id}</span>
                <span className="note-col">
                  {s.from_version ? `v${s.from_version} → v${s.to_version}` : `v${s.to_version}`} ·{" "}
                  {s.reason}
                </span>
                <span className="dim">{s.keys} keys</span>
                <button onClick={() => restore(s.id)} disabled={!!busy}>
                  Restore
                </button>
              </li>
            ))}
          </ul>
        </details>

        <details className="schema">
          <summary>
            Data shapes · {keys.length - unprotected.length}/{keys.length} protected
          </summary>
          <p className="note">
            A module declares the fields it owns. Anything it does not declare is
            preserved when it writes, so an older version cannot strip a field a newer
            one added. Undeclared keys are replaced wholesale.
          </p>
          <ul>
            {keys.map((k) => (
              <li key={k.key}>
                <span className="note-col">{k.key}</span>
                <span className={k.protected ? "good" : "bad"}>
                  {k.shape ?? "unprotected"}
                </span>
              </li>
            ))}
            {keys.length === 0 && <li className="dim">No app data yet.</li>}
          </ul>
        </details>

        <details className="sourceview">
          <summary>Source · {source.length} bytes</summary>
          <pre>{source}</pre>
        </details>
      </div>
    </div>
  );
}
