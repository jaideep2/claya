# claya

A macOS app that rewrites its own interface from a chat prompt.

What it is trying to be, and what that rules out: **[PRINCIPLES.md](PRINCIPLES.md)**.
Visual decisions: [DESIGN.md](DESIGN.md).

## Where this is

- [x] **M1 — Skeleton.** Two Tauri windows with a real privilege split.
- [x] **M2 — Runtime loader.** TSX compiled and mounted at runtime, no bundler.
- [x] **M3 — Persistence.** Modules + versions in SQLite via rusqlite.
- [x] **M4 — The loop.** Claude call in Rust, chat in the shell.
- [x] **M5 — Safety.** Probe-gated commit, auto-rollback, data snapshots.
- [x] **M6 — Schema declarations.** Modules declare what they own; the rest is preserved.
- [x] **M7 — Ship.** Hardened runtime verified, sign + notarize scripted.

## The drawer

The chat docks flush against the canvas's right edge and slides, IDE-style. It is
still a **separate window**, and that is not an implementation detail — Tauri scopes
capabilities by window label, so moving the chat inside the canvas webview would
hand `save_module`, `set_active_version`, and the API key to model-authored code.
Multi-webview would allow one window with the boundary intact, but it is still
behind Tauri's `unstable` flag with open positioning and resize bugs.

So: two windows, and Rust keeps them glued. `dock_drawer` repositions the shell on
every canvas move and resize. The window appears instantly at the docked position
and the *content* transitions in — a GPU-backed CSS transform, rather than moving a
window frame per frame, which janks.

**The toggle lives in the tray, not in the canvas.** The control you use to undo a
bad generation must not be something a bad generation can hide.

## The two windows

| | `shell` | `canvas` |
|---|---|---|
| Written by | You, by hand | The model |
| Trust | Privileged | Untrusted |
| Capability | everything below | read its own source + app state, nothing more |
| Survives bad codegen | Yes | No — that's the point |

The split is enforced in Rust, not by convention. `src-tauri/build.rs` registers the
app's commands into the ACL via `AppManifest::commands()`; without that step Tauri
makes **every** command callable from **every** window, and the isolation is decorative.

The canvas proves this at boot: it attempts `invoke("probe_canvas")` and expects to be
rejected. The result surfaces in the shell as *Isolation: enforced*. If it ever reads
BREACHED, stop and fix it before writing another line.

## The module store

Modules live in SQLite (`~/Library/Application Support/com.claya.app/claya.db`),
not on disk. Every edit is an `INSERT`; `active_module` is a pointer at whichever
version is live. So a swap is a transaction, rollback is a pointer move, and **no
version is ever destroyed** — which is what M5's auto-rollback will stand on.

```
modules(name, version, source, parent_version, note, created_at)
active_module(name, version)        -- which one is live
kv(key, value, updated_at)          -- app state, behind host.state
```

`parent_version` records what a version branched from, so rolling back to v1 and
editing gives you a v3 whose parent is v1 — the lineage survives, rather than the
history reading as a straight line that never happened.

The seed module is `include_str!`'d into the binary, so a fresh install has something
to run before the first model call.

### Who can call what

| Command | shell | canvas |
|---|:--:|:--:|
| `load_active_module`, `kv_get` | ● | ● |
| `kv_set` | | ● |
| `save_module`, `set_active_version`, `list_versions`, `load_version` | ● | |
| `probe_canvas`, `reload_canvas` | ● | |

The canvas reading its own source is harmless and lets it boot itself. Writing a
version is the line it must never cross. `src-tauri/src/db.rs` has tests covering
append, rollback, branch lineage, and the kv size cap: `cargo test`.

## How runtime loading works

`src/canvas/loader.ts`:

0. **Import check** runs first, against the registry. It has to: Sucrase *elides*
   imports whose bindings are unused, because it cannot distinguish them from
   type-only imports. Skip this and a forbidden import is silently dropped, then
   explodes later somewhere that tells the model nothing. Dynamic `import()` is
   rejected outright — it would bypass the registry entirely.
1. **Sucrase** strips TypeScript and JSX, and rewrites ESM `import` to CJS `require`.
   ~1ms, no network, no bundler. This is what makes the app modifiable *after* it ships.
2. **`new Function`** evaluates the result, with `require` bound to a fixed registry
   (`src/canvas/main.tsx`). A module reaches exactly what we hand it and nothing else.
3. Failures are reported by phase — `imports` / `transpile` / `evaluate` / `contract` /
   `render` — so M4 can tell the model whether it wrote code that doesn't parse, doesn't
   run, or reached for something it isn't allowed. `npm run test:loader` covers all five.

### What the registry does not do

It constrains **imports**, not **globals**. Code inside `new Function` still sees `window`,
`fetch`, `localStorage`. Two things carry that weight instead: the CSP (`default-src 'self'`
covers `connect-src`, so there is no egress to any remote host) and the canvas capability,
which grants no privileged command. Tightening this further — an iframe sandbox — is an
M5 question, not something the registry solves.

This needs `script-src 'unsafe-eval'` in the CSP (`src-tauri/tauri.conf.json`). That is a
deliberate trade: the app is a code-execution engine, so the capability boundary does the
security work instead of the CSP.

**Module contract:** default-export a React component. Available imports: `react`, `@host`.

## Verification is driven from Rust

`probe_canvas` uses `Webview::eval_with_callback` (Tauri 2.11+) to run a probe and read
the answer back. Deliberately *not* an event the canvas emits — the module under test
doesn't get to vote on its own health. Wedged code sends nothing, the 2s timeout fires,
and that is the answer. M5 turns this into the commit/rollback gate.

## The model loop

`src-tauri/src/llm.rs` calls `POST /v1/messages`, forcing a
single `edit_module` tool call (`strict: true`) so the reply is always a complete
new source plus a note and an explanation — never prose, never a diff. The model is
switchable from the shell (Sonnet 5 by default, Opus 5 available) and stored in
`settings`.

**The API key lives in the macOS Keychain and is read only by Rust.** It never
crosses into a webview.

### Why the Keychain used to prompt on every launch

macOS binds a Keychain item's ACL to the **code signature of the binary that reads
it**. `cargo build` emits a new signature every build, so each dev binary is a
different app as far as the Keychain is concerned, and gets re-prompted. Clicking
"Always Allow" only holds until the next rebuild.

Three changes reduce it:

- **The key is read once per process, not once per request.** A long session no
  longer prompts per message. This cannot help *across* rebuilds — nothing can,
  short of a stable signing identity — but it removes the repeated asking within
  one run.
- **`api_key_status` no longer reads the Keychain.** Reading is what triggers the
  prompt, and doing that on every launch just to render a status row is what made it
  nag. A marker in `settings` answers the question; the Keychain is touched only when
  a request is actually being sent.
- **`ANTHROPIC_API_KEY` takes precedence when set.** For development, export it and
  the Keychain is never consulted at all.

A debug build prints the `export` line at startup when it falls back to the
Keychain, because the fix is one command and the annoyance recurs on every rebuild.

A signed release build (M7) has a stable identity, so it authorises once and stays
authorised. This is purely a development-time cost. In a normal app that is good hygiene; here it is the
requirement, because one of those webviews runs code this very model wrote.

Two context decisions worth keeping:

- **History replays prompts and explanations, never past sources.** The current
  source is in every request already; replaying old ones would eat the context
  budget within a handful of edits and teach the model nothing new.
- **`propose_change` does not write.** It returns a proposal; the shell applies it
  through `save_module` — the same path the manual editor used. One write path
  means M5 has exactly one place to install the verify-then-commit gate.

The contract the model must satisfy (`CONTRACT` in `llm.rs`) is a mirror of what
`loader.ts` enforces. **If you widen the registry, widen the contract in the same
commit** — otherwise the model writes imports that compile in its head and are
rejected at runtime.

## The gate (M5)

`apply_and_verify` is the single write path — chat and any manual edit both go
through it — and it runs as one Rust operation so it cannot be left half-applied
by a closed window or a dropped promise:

1. snapshot the data, append the new version, move the pointer
2. reload the canvas
3. **settle** — poll until the module either mounts or reports a definite failure
4. healthy → commit. Otherwise move the pointer back and reload.

Step 3 is the part that is easy to get wrong. Straight after a reload the probe is
not installed yet; that is *still loading*, not *failed*. Treating it as failure
would roll back every healthy swap. So `settle` waits for one of two definite
answers — mounted, or booted-with-an-error — and calls anything still ambiguous at
the 6s deadline a timeout failure. That last case is what catches a render loop.

A failed version **stays in the table**. Only the pointer moves. The failure is also
appended to the chat transcript, so the next prompt carries it and the model can see
what it broke without you retyping it.

### Background throttling is load-bearing

Both windows set `"backgroundThrottling": "disabled"` in `tauri.conf.json`. This is
not a nicety. WKWebView suspends JavaScript in occluded windows, and the health
probe runs *inside* the canvas — so with throttling on, applying a change while the
app sits behind another window makes every probe time out, and the gate rolls back
perfectly good modules. It fails exactly when you are not looking at it.

Verify the gate actually fires:

```bash
CLAYA_VERIFY_ROLLBACK=1 npm run tauri dev
```

It applies a module that cannot compile and prints whether the pointer came back.
The deliberately broken version is left in your history — that is the evidence.

## Switching versions, and what happens to your data

The pointer moves in both directions, so the verb is **switch**, not revert.

`kv` is **shared and live across every version**. That is deliberate: it is what
lets a field a newer module introduced still be there when you keep going forward.
Namespacing data per version would avoid every conflict and also mean your todos
vanish the moment the app changes shape — which defeats the point.

The cost of sharing is real, and worth stating plainly: **an older module can strip
a field it does not know about.** Switch to a version that predates due dates, let
it rebuild its objects as `{id, title, done}`, and the `due` field is gone on its
next write. Nothing warns you.

So every version change snapshots the whole `kv` first:

```
kv_snapshots(id, from_version, to_version, reason, taken_at)
kv_snapshot_entries(snapshot_id, key, value)
```

Snapshots are taken at **version boundaries, not on every write** — bounded by how
often you switch rather than how often the app saves, and taken at exactly the point
where the shape can change underneath the data. Restoring one snapshots the current
state first, so a restore is itself undoable.

That is recovery. **M6 adds prevention.**

### You may only destroy what you declare

A module exports the shape of the data it owns:

```tsx
export const schema = {
  todos: { type: "record-list", key: "id", fields: ["id", "title", "done"] },
};
```

The canvas registers this with Rust *before its first render* — modules write on
mount, so a late registration means that first write replaces instead of grafts.
Then on every `kv_set`, Rust grafts back any stored field the writer neither
declared nor supplied. An older module that rebuilds its todos as
`{id, title, done}` no longer destroys `due`: it never declared `due`, so `due`
is not its to remove.

The rule stays intuitive in both directions:

- a field you **declare** is yours — change it, remove it, it obeys you
- a field you **don't declare** belongs to another version and survives your write
- a **row you drop** from a `record-list` is an intentional delete and stays deleted
- a key with **no declaration** is replaced wholesale, and the shell shows it as
  unprotected

The declaration comes from untrusted code, which is fine: declaring more fields
only lets a module destroy more of *its own* data — it cannot reach another key,
and the M5 snapshots still cover the case where it lies.

Shapes: `{type: "scalar"}`, `{type: "record", fields}`, `{type: "record-list", key, fields}`.

## Living on the machine

Two separate questions hide behind "does it persist?"

**Your data and your edits already do.** Modules, version history, chat, and app
state are in `~/Library/Application Support/com.claya.app/claya.db`. Quit,
relaunch, and every version you evolved is still there. This is a real benefit of
storing modules in SQLite rather than in the bundle: replacing the `.app` with a
new build does not touch what the app became.

**The app itself is not installed yet.** `npm run tauri build` leaves a working
bundle at `src-tauri/target/release/bundle/macos/Claya.app`; drag it to
`/Applications` and it behaves like any other app. It runs unsigned on *this* Mac
because a locally built binary carries no quarantine attribute.

Giving it to anyone else needs M7: an Apple Developer account, a Developer ID
Application certificate, `codesign` with the hardened runtime, `notarytool`, and
`xcrun stapler`. Two wrinkles specific to this app:

- **Hardened runtime vs. `unsafe-eval`.** JS runs in WKWebView's own process, so
  the app should not need a JIT entitlement — but verify it on a signed, notarized
  build early rather than on release day. This is the one thing most likely to bite.
- **Don't enable App Sandbox casually.** Not required outside the Mac App Store,
  and it would need entitlements for the Keychain and network access.

For shipping new *engine* versions (Rust, loader, shell) use `tauri-plugin-updater`.
Self-modification covers the UI layer only; the shell around it still updates the
ordinary way.

## Identity and integrity

The shell shows a build line like `0.1.0 · a3f81c92`. Two halves, guarding
different things:

- **`0.1.0` — the engine** (Rust, loader, shell). Its real integrity guarantee is
  the **code signature**, not a hash we compute. The OS verifies it on every launch
  and does it better than we could.
- **`a3f81c92` — the app**, as the head of a hash chain over the module history:

  ```
  hash(v) = sha256(hash(v-1) ‖ name ‖ version ‖ source ‖ note)
  ```

  Each version folds in its predecessor, so the head fingerprints the *entire*
  history. Edit a stored version with a `sqlite3` shell and its hash stops matching
  — and because every later hash folded the original in, re-stamping the row you
  edited does not help: the break just moves downstream. Verified on launch
  (`[integrity] ok=…`) and shown in the shell.

**No operation ever overwrites a hash it has already written.** Reset deletes a
suffix, which leaves every surviving hash byte-identical (`git reset --hard`
semantics), and middle-removal is not offered at all. So a hash you recorded
months ago still verifies, unqualified.

Scope is deliberate. "Tampering" cannot mean "a module changed" — that is the whole
feature. It covers **history**, which is at risk because it sits in a SQLite file
anyone can open. It does not cover app data, which changes constantly and would
produce noise, nor the binary, which is codesign's job.

## Shipping

```bash
./scripts/release.sh --adhoc   # verify the hardened runtime locally, no Apple account
./scripts/release.sh           # Developer ID sign + notarize + staple
```

Full mode reads `SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_TEAM_ID`, and
`APPLE_PASSWORD` (an app-specific password, not your Apple ID password).

**The hardened runtime question is settled.** Compiling TSX and evaluating it with
`new Function` works under `--options runtime` with **no JIT entitlement** — because
the JavaScript runs in WKWebView's own content process, which carries its own. This
was the largest open risk in the whole design and `--adhoc` checks it in about a
minute, before an Apple account is anywhere near the process. Do not add
`allow-jit` or `allow-unsigned-executable-memory` speculatively: each one widens the
attack surface of a process that deliberately executes generated code.

App Sandbox is intentionally off — not required outside the Mac App Store, and it
would need extra entitlements for Keychain access.

### The updater

Wired. `tauri-plugin-updater` plus a `plugins.updater` block; the shell has an
**Engine** row that checks the feed and installs on demand.

Self-modification covers the UI layer only. The engine — Rust, `loader.ts`, the
shell, the capability grants, the model contract — can only change by shipping a
new binary, and that is what the updater is for. Modules and app data live in
Application Support, so **an engine update does not touch what your app became**.

Checking and installing are deliberately separate, and installing is never
automatic: an engine update is the one change in this app that a version switch
cannot undo.

**The public key in `tauri.conf.json` is a placeholder.** Its private half was
generated outside the repo and destroyed, so nothing can be signed against it.
`scripts/release.sh` refuses to build while it is still there — because a wrong
key fails *silently*, looking exactly like "no update available":

```bash
npx tauri signer generate -w ~/.tauri/claya.key   # back this up offline, first
# paste the .pub contents into plugins.updater.pubkey
rm src-tauri/.placeholder-pubkey
export TAURI_SIGNING_PRIVATE_KEY=~/.tauri/claya.key
```

The endpoint should be `updates.claya.app`, per `plans/05-namespace.md` — it is
still configured as `updates.claya.app` from before the rename. That host and the
bundle identifier are both compiled into every shipped binary, so per
`02-milestones.md` **the rename has to land before anything ships**: the endpoint
needs updating and the identifier is still `com.claya.app`.

## Running

```bash
npm install
export ANTHROPIC_API_KEY=sk-ant-...   # optional; avoids Keychain prompts in dev
npm run tauri dev
```

Rust was installed here with `rustup --no-modify-path`, so `~/.cargo/bin` is not on
an interactive shell's `PATH`. The npm scripts prepend it themselves, so nothing
needs setting up — but a bare `cargo …` in your own terminal will not resolve
unless you add `. "$HOME/.cargo/env"` to your shell profile.

## Components and themes

Modules build their interface from **Radix Themes**, imported as `@ui`. It was
chosen over Astryx (StyleX, compile-time), HeroUI (Tailwind peer) and shadcn
(copy-the-source) for one reason: **we have no build step at runtime.** Radix ships
a single prebuilt `styles.css`, so Vite inlines it and the components resolve from
the registry with nothing to compile.

The other reason is the split:

```tsx
<Theme accentColor="crimson" radius="large" appearance="dark">
  <YourModule />
</Theme>
```

**The theme wraps the module rather than living inside it.** So changing it
re-skins every version in the history at once — including modules written before
the theme existed — with no regeneration and no model call. The contract forbids
modules from rendering `<Theme>` or hard-coding a hex colour, precisely so the
user's choice reaches their interface.

Rust pushes theme changes into the canvas through `window.__set_theme` rather than
a Tauri event. Granting the canvas event access would let model-authored code emit
`drawer:request-close` and collapse the drawer — capturing the escape hatch
`PRINCIPLES.md` says it must never touch.

~50 components, listed exhaustively in the contract. That number is a deliberate
ceiling: every component a model can name is one it can hallucinate a prop for, and
each mistake costs a gate failure and a round trip.

## Templates

Four hand-written starters — todo, notes, tracker, board. A fresh install shows a
picker above the chat; afterwards they live in the **Templates** panel.

Hand-written rather than generated on purpose: a template that fails the gate on
first run is unrecoverable from inside the app, so they must never fail.
`npm run test:templates` compiles every one through the real loader and asserts it
declares a schema; `npm run test:e2e` applies all four through the actual gate.

Picking one **branches**: it becomes a new version, your current one stays in the
history, and your data is left alone — schema grafting absorbs any shape mismatch.
Sample rows are seeded only when no key carries content yet, so a fresh install
looks alive and an existing one never gets demo data mixed into real work.

To see the first-run experience without touching your history:

```bash
CLAYA_DB=/tmp/fresh.db npm run tauri dev
```

## Tests

```bash
npm test        # loader + templates + Rust — run this after every change
npm run test:e2e   # boots the real app against a throwaway DB
npm run test:all   # both
```

`npm test` is 7 loader checks (every failure phase), 4 template checks, and 27 Rust checks covering the
module store, rollback, branch lineage, kv snapshots, schema grafting, and the
integrity chain — including that a tampered row cannot be re-hashed in isolation.
Sub-second, no GUI.

### Driving the loop from the command line

```bash
CLAYA_PROMPT="make the background black" npm run tauri dev
```

Runs one full turn — `propose_change` → `apply_and_verify` — through the exact path
the chat box uses, and prints the note, the model's explanation, and the gate
result. It exercises the model call, the contract, the gate and the rollback
together, which `npm run test:e2e` deliberately does not: that suite must stay free
and offline, and this one costs a real API call every time.

`npm run test:e2e` boots the actual app with `CLAYA_DB` pointed at a temp file,
applies a module that cannot compile, and asserts the gate rolled it back and the
canvas recovered. It runs against a throwaway database on purpose: an earlier
version of this check left six junk versions in real history.

Watch stdout for `[self-test]` — it reports mount status, isolation, and DOM node count
about 2s after launch.

Release build: `npm run tauri build`. Worth doing early and often — it is the only way to
know runtime loading works with no Vite present.

## Layout

```
src/shell/          hand-written, privileged. Never let the model touch this.
                    Editor + version history today; chat box in M4.
src/canvas/
  loader.ts         Sucrase + new Function
  host.ts           the API model-authored code may use — kv_get/kv_set into SQLite
  main.tsx          registry, error boundary, probe, isolation self-test
  seed-modules/     starting app source, compiled into the binary via include_str!
src-tauri/
  build.rs          registers commands into the ACL — load-bearing
  capabilities/     per-window permission grants
  src/db.rs         module store, version history, kv (+ tests)
  src/lib.rs        commands
```
