# Bugs found while building

Kept because each one was invisible until something specific was tested, and each
would have come back.

## Sucrase elides unused imports (M2)

Sucrase cannot distinguish an unused import from a type-only import, so it drops
it. A forbidden `import fs from "node:fs"` therefore vanished silently and the
registry guard never fired — the model got no feedback until it later *used* the
import, at a point that explained nothing.

**Fix:** check imports *before* transpiling, and reject dynamic `import()` outright
since it bypasses the registry entirely. `npm run test:loader`.

## Background throttling suspends the health probe (M5)

WKWebView suspends JavaScript in occluded windows. The probe runs inside the
canvas, so with the app behind another window every probe timed out and the gate
rolled back perfectly good modules — failing precisely when nobody was watching.

**Fix:** `"backgroundThrottling": "disabled"` on both windows.

## `settle` read the outgoing document (M5)

For a few milliseconds after `reload()` the old page is still alive and still
answering. `settle` accepted its verdict. In the rollback direction that was
cosmetic; in the forward direction it meant **a broken module could be committed
because the module it replaced was healthy** — the gate silently not gating.

**Fix:** each page load generates a `boot` token; `settle` ignores any report
carrying the pre-reload token.

## The Keychain prompted on every launch (M5)

macOS binds a Keychain item's ACL to the code signature of the reading binary.
`cargo build` emits a new signature each build, so every dev binary looked like a
different app. Worse, `api_key_status` read the Keychain on startup purely to
render a status row — so it nagged before you did anything.

**Fix:** status reads a marker in `settings` instead; the Keychain is touched only
when a request is actually sent; `ANTHROPIC_API_KEY` takes precedence when set.

## Editing an applied migration is a no-op (M6)

A `failed` backfill was added by editing the `current < 6` block — which had already
run, so `user_version` was 6 and the block never executed again. The column existed;
the backfill silently did not. Re-added as migration 7.

**Rule:** a migration that has shipped is immutable. Corrections go in the next one.

## "Empty store" was the wrong test for seeding samples (M8)

Template sample data was seeded only when `kv` had no rows. But the app boots a
module that writes its own empty key on mount, so the store is never actually
empty by the time anyone picks a template — the samples silently never landed, and
every unit test passed because none of them booted the app.

**Fix:** the predicate is *no key carries content* (`value NOT IN ('[]','{}','')`),
not *no keys exist*. Caught by `npm run test:e2e`, which is the only check that
runs the real app.

## `cargo` was not on the user's PATH (M8)

rustup was installed with `--no-modify-path`, and every command in the build
session was prefixed with `PATH="$HOME/.cargo/bin:$PATH"`. So the toolchain worked
throughout development and `npm run tauri dev` failed from a clean shell with an
opaque `cargo metadata … No such file or directory`.

**Fix:** the npm scripts prepend `~/.cargo/bin` themselves, so the repo works from
a clean shell without anyone editing dotfiles. Verified by running the failing
command in a shell where `which cargo` finds nothing.

**Rule:** if the working session needs an environment tweak to function, that tweak
belongs in the repo, not in the operator's shell.

## The shell reported "unhealthy" forever (M9)

The shell probed the canvas once, on mount — which races a canvas that is still
loading. The probe came back not-yet-mounted, nothing re-checked, and the status
chip stayed wrong for the life of the window while the startup self-test reported
the canvas perfectly healthy.

**Fix:** `probe_canvas` now goes through `settle`, which waits for a *definite*
answer (mounted, or a definite failure) rather than sampling once. The common case
is still immediate.

**Rule:** any read of the canvas's state races its load. `settle` exists for that;
use it rather than `probe_once` anywhere a human will read the result.

## The test registry drifted from the real one (M9)

`test/templates.test.mjs` built its own registry object, so adding `@ui` to the app
left the tests compiling against a registry that no longer existed. Every template
failed with `Cannot import "@ui"` — a false alarm, but next time it would be a
false pass.

**Rule:** the test registry must mirror `REGISTRY` in `src/canvas/main.tsx`
exactly. A test fixture that drifts from production proves nothing.

## Applying the rebase penalty to a reset (M10)

Reset was written to null every hash and re-derive the chain, and the docs claimed
this cost tamper-evidence. Both were wrong. Truncation removes a *suffix*, so no
surviving hash folded in a row that went — `h1` stays `h1` however many later
versions are deleted, exactly as `git reset --hard` leaves the surviving commits'
SHAs alone. Only *middle*-removal invalidates what follows.

The re-stamp was a no-op that looked like a safeguard, and would have silently
laundered a genuine corruption. `reset_to` now touches no hashes and **verifies**
afterwards, erroring if the chain does not hold.

**Rule:** never recompute a checksum to make a verification pass. If it does not
verify, that is the finding.

## Reset deleted a version the user never saw (M10)

A reset to v1 removed three versions, not the two the history showed. A generation
had completed between the last refresh and the reset, so the confirm dialog said
"delete 2" while the truth was 3. The extra version was destroyed and, unlike the
data, versions are not recoverable.

The obvious guard — disabling Reset while a generation is in flight — would not
have helped: the generation had already *finished*. The problem was acting on a
stale view, not a concurrent one.

**Fix:** `reset_to` takes the chain head the caller believes and refuses if the
real head differs — a compare-and-swap. The confirm promises to delete N versions,
so deleting more than N without re-confirming is the bug, whatever the cause.
Scripted resets pass `None`, since a script has no screen that could be stale.

**Rule:** any confirm that names a quantity has to re-check that quantity at the
moment it acts. The number on the button is a promise.

## The rename lost the history and stranded the key (M11)

Renaming to `com.claya.app` moved the data directory, and the old one was gone by
the time anyone looked — so v2 and v3 (the "fancy" redesign and the calendar) were
lost. `02-milestones.md` had specified the migration (`VACUUM INTO` from the old
path on first launch, leave the original in place) and the rename shipped without
it.

The Keychain item was stranded the same way: stored under `com.selfbuild.dev`,
unreadable by a binary asking for `com.claya.app`.

**Fixes:** `get_key` falls back to the legacy service and promotes the key onto the
current name. The promotion also runs once at startup, because the shell's key gate
blocks sending — the lazy path alone would never have been reached.

**Rule:** an identifier change is a data migration. Write and test the migration
before the rename lands, not after — by then the source may be gone, and nothing in
the build will tell you.

## keyring 4 renamed its backend features

`features = ["apple-native"]` does not exist; `default = ["v1"]` already pulls in
the Apple keychain store. Compile-time failure, cheap, noted so it is not repeated.
