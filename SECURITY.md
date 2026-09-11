# Security Policy

Claya compiles and executes model-authored code at runtime, by design. That
makes its security boundary unusual, and worth stating precisely — both so real
issues get reported, and so time is not spent on things that are the feature.

## Reporting a vulnerability

**Use GitHub's private vulnerability reporting** — the *Report a vulnerability*
button under the Security tab. It is private, requires no setup on your side,
and reaches the maintainer directly.

Email `info@claya.app` if you prefer.

Please do not open a public issue for a suspected vulnerability. You will get an
acknowledgement within a few days; this is a small project, so please be patient
with the follow-up. If you do not hear back within a week, feel free to nudge via
a public issue that says only that you are waiting on a security report.

## The design, in one paragraph

Claya runs two windows. The **shell** is hand-written and privileged: it holds
the chat, the API key, and every command that can write. The **canvas** renders
code the model generated, and is granted almost nothing — it may read its own
source and read/write its own app data, and that is all. The split is enforced by
Tauri capabilities scoped per window label, registered into the ACL by
`src-tauri/build.rs`. The canvas proves the boundary on every launch by
attempting a privileged call and expecting rejection.

## In scope

Anything that crosses that boundary, or forges the record of what happened:

- **Capability escape** — model-authored code in the canvas reaching a
  shell-only command (`save_module`, `set_active_version`, `propose_change`,
  `reset_to_version`, `set_theme`, …), by any route including IPC, events,
  prototype pollution, or the module registry.
- **API key disclosure** — any path by which the Anthropic key, which lives in
  the macOS Keychain and is read only by Rust, becomes reachable from a webview,
  a log, a crash report, or disk.
- **Integrity forgery** — producing a module history whose hash chain verifies
  but does not reflect what was actually written. The chain is append-only and
  no operation overwrites a hash it has already written; a counterexample is a
  vulnerability.
- **Escaping the module registry** — importing anything beyond `react`, `@host`
  and `@ui`, or otherwise obtaining a capability the registry does not hand out.
- **Update channel attacks** — anything that gets an unsigned or substituted
  binary past the updater's signature check.
- **Data loss that bypasses the snapshot rule** — a destructive path that
  removes app data without first snapshotting it.

## Not in scope

These are known, deliberate, and documented:

- **The model generating bad, broken or ugly code.** That is the premise. A
  version that fails to compile or mount is rolled back automatically by the
  gate, and every version is kept so the user can switch back.
- **`unsafe-eval` in the CSP.** Claya is a runtime code-execution engine; the
  capability boundary does the security work, not the CSP. See `README.md`.
- **The canvas having access to `window`, `fetch`, `localStorage`.** The registry
  constrains *imports*, not globals. Egress is constrained instead by
  `default-src 'self'`, which covers `connect-src`.
- **Reset deleting version history.** Reset is destructive on purpose, behind a
  confirm that names the count. See `PRINCIPLES.md`.
- **Prompts that make the model write something the user did not want.** Claya
  shows every generated version, keeps all of them, and reverts the ones that do
  not run. Persuading the model is not a vulnerability in Claya.

If you are unsure which side of the line something falls on, report it. A
misfiled report costs a message; an unreported escape costs more.

## Supported versions

Claya is pre-1.0 and unreleased. Only `main` is supported. Once releases begin,
fixes will land in the latest release and be pushed through the updater.
