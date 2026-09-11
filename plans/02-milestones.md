# Milestones

| | Milestone | State |
|---|---|---|
| M1 | Skeleton — two windows, capability split | done |
| M2 | Runtime loader — TSX compiled and mounted with no bundler | done |
| M3 | Persistence — modules + versions in SQLite | done |
| M4 | The loop — Claude call in Rust, chat in the shell | done |
| M5 | Safety — probe-gated commit, auto-rollback, data snapshots | done |
| M6 | Schema declarations — prevent field loss, not just recover | done |
| M7 | Ship — hardened runtime verified, sign + notarize scripted, updater wired | done |
| M8 | Templates — four hand-written starters, picker on first run | done |
| M9 | Components + themes — Radix Themes as `@ui`, live theming | done |

The chat became a docked, sliding drawer during M6 (still a separate window — see
`03-decisions.md`), with the toggle in the tray.

## Outstanding in M7

One step only the maintainer can take:

- **Generate the updater keypair** and paste the public half into
  `plugins.updater.pubkey`. `scripts/release.sh` refuses to build until the
  placeholder is replaced and `src-tauri/.placeholder-pubkey` is removed. Back the
  private key up offline before anything else — see `PRINCIPLES.md`; it is the one
  secret whose loss cannot be recovered from.

The rename to **claya** has landed: identifier `com.claya.app`, feed
`updates.claya.app`. Both are compiled into shipped binaries, and nothing has
shipped, so they were free to set. See `03-decisions.md` for why they must not
move again.

## Known gap

**Nothing automated exercises the model call.** `test:e2e` covers the loader, the
gate, rollback and templates, but stops short of the API — deliberately, so the
suite stays free and offline. `CLAYA_PROMPT=…` drives a real turn by hand when
the contract changes, which is the moment generation quality can silently regress.
A recorded-fixture test over `llm::propose` would close it without spending money.

## Later

Not scheduled. Recorded so they are not re-derived from scratch.

- ~~Starter templates~~ — **done, see M8.** Two deviations from the sketch,
  both deliberate:

  - **Constants, not a `templates` table.** A table would need refreshing on every
    engine update and could go stale against the binary reading it. Compiled-in
    constants cannot. Revisit only if templates become user-authored.
  - **"Seed samples into an empty store" needed a better definition of empty.**
    The app boots a module that writes its own empty key on mount, so `kv` always
    has rows by the time anyone picks a template — the predicate is *no key
    carries content*, not *no keys exist*. The first version silently seeded
    nothing; `npm run test:e2e` caught it.
- **Multi-module apps.** One `app` module today. Real apps want several files with
  the registry resolving between them.
- **Diff preview before apply.** Show what changed and let the user reject before
  the swap, rather than only after.
- ~~Model-authored stylesheets~~ — **superseded by M9.** Modules compose Radix
  Themes components and the user owns the theme, which is a better answer than
  letting each generation invent its own CSS.
- **Context budget.** History replays prompts and explanations only. Once modules
  get large, the current source itself needs summarising.
- ~~"Reset to" alongside "Switch to"~~ — **done, and the decision was reversed.**

  The earlier plan here said reset should *restore that version's data* rather
  than wipe, on the grounds that a wipe destroys user work to undo a code change.
  That was overturned deliberately: keeping the abandoned branch is what made the
  history unusable. Resetting to v1 and editing produced v11, not v2.

  **Reset now truncates.** Versions after the target are deleted, `kv` is cleared,
  and the next edit is N+1. The data is snapshotted first so a mistaken reset is
  recoverable; the versions are not. `PRINCIPLES.md` was updated to match rather
  than left contradicting the code.

- ~~Glue the two windows together for Mission Control~~ — **done.** The shell is
  a child window of the canvas (`"parent": "canvas"`), so they raise and travel as
  a unit and the drawer gets no Mission Control tile of its own. The capability
  boundary is unaffected: parenting is window ordering, and capabilities are scoped
  by window label.

  <details><summary>original note</summary>

  F3 currently surfaces
  the shell and the canvas as two unrelated windows, and raising one leaves the
  other behind. They should travel as a unit.

  Fix: make the shell a **child window** of the canvas. `WindowConfig` has a
  `parent` field (confirmed in the Tauri 2.11 config schema), so it is
  `"parent": "canvas"` on the shell entry. macOS child windows are ordered above
  the parent, move with it, raise with it, and do not get their own Mission
  Control tile.

  Two things to check when doing it:

  - **The capability boundary is unaffected.** Capabilities are scoped by window
    *label*, and parent/child is only window ordering — the shell keeps its
    commands, the canvas keeps none of them. This does not walk back the two-window
    decision in `03-decisions.md`.
  - **It probably overlaps `dock_drawer`.** macOS moves a child window with its
    parent automatically, so the per-move repositioning may become redundant, or
    may fight the automatic behaviour. Expect to simplify or remove part of it
    rather than keep both.

  </details>
