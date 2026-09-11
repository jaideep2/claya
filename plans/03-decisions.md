# Decision log

Why things are the way they are, so they are not re-litigated.

## Two windows, not two webviews

Tauri 2 supports multiple webviews in one window, and capabilities scope per
webview — which would be the ideal primitive. It is behind the `unstable` flag with
open bugs on positioning, resize, and only-the-last-child rendering. Two windows are
stable and grant the same isolation, so the chat drawer is a *second window
positioned against the first*, not a second webview inside it.

## `build.rs` is load-bearing

By default Tauri makes every `#[tauri::command]` callable from **every** window.
`AppManifest::commands()` in `build.rs` is what puts them behind the ACL. Delete
that block and the isolation silently becomes decorative — nothing warns you.

## The canvas may read its own source, never write a version

`load_active_module` and `kv_get`/`kv_set` are granted. `save_module`,
`set_active_version`, `list_versions`, `propose_change` are not. Reading its own
source is harmless and lets the canvas boot itself; writing a version is the line.

The isolation self-test probes `list_versions` — deliberately a *read-only*
shell command. Probing the write path would corrupt the store it is checking if
isolation were ever broken.

## Verification is driven from Rust, and is generation-aware

The probe runs via `eval_with_callback` from the privileged side, so a wedged
module simply fails to answer and the timeout is the verdict. Two subtleties, both
found the hard way:

- **Background throttling must be disabled.** WKWebView suspends JS in occluded
  windows, so with throttling on the gate rolled back healthy modules whenever the
  app was not frontmost.
- **The probe report carries a `boot` token.** For a few milliseconds after
  `reload()` the outgoing document still answers. Without the token, `settle` read
  the *previous* module's verdict — which in the forward direction meant a broken
  module could be committed on the strength of the old one's healthy report.

## App data is shared across versions, not namespaced

Namespacing `kv` per module version would prevent every conflict and also make the
user's data vanish each time the app changed shape. Sharing keeps data flowing
forward; the cost is that an older module can strip a field it does not know about.

- **M5 made that recoverable**: every version change snapshots the whole store,
  bounded by switches rather than writes.
- **M6 makes it preventable**: a module declares which fields it owns, and the
  engine preserves everything it did not declare. *You may only destroy what you
  declare.*

## One write path

`apply_and_verify` is the only way a version becomes live — chat and any manual
edit both go through it, and it runs as a single Rust operation so it cannot be
left half-applied. That is what makes the M5 gate a gate rather than a convention.

## Model choice is a setting, not a constant

Sonnet 5 by default, Opus 5 available, stored in `settings` and switchable from the
shell. The refusal-`fallbacks` parameter was dropped when moving off Opus, as it is
documented for the Opus/Fable tier.

## Deleting history: hidden in production, truncated by Reset

`PRINCIPLES.md` forbids destroying history. The complaint that tested it was real
— six junk versions from the gate self-test cluttering the list — and it resolved
into two different answers for two different audiences:

- **Production hides.** A filter on the Versions panel; the `failed` flag already
  existed. Nothing is lost, and the principle is untouched.
- **Reset truncates.** The single deletion path, available everywhere.

*Dev-only compaction was built and then removed.* It deleted failed versions from
the middle of the history, which forces the rows after them to be re-hashed. Once
it was clear that Reset (a suffix removal) needs no re-hashing at all, keeping
compaction meant carrying the one operation in the app capable of rewriting
history — for a convenience the "hide failed" filter already covers.

The technical argument matters as much as the philosophical one: the integrity
chain folds each hash into the next, so removing a version invalidates every hash
after it. Compaction therefore has to re-stamp the chain, which destroys
tamper-evidence for the rows it keeps. Deletable history and tamper-evident history are mutually exclusive **only for
middle-removal**. Suffix removal is free. Since suffix removal is the only kind
offered, the app never rewrites a hash it has written — see
`db::immutability_tests`.

## Radix Themes for components, chosen on one constraint

The runtime has no build step, which eliminates most of the field:

| | Why not |
|---|---|
| Astryx (Meta) | StyleX is compile-time |
| HeroUI | peers on `tailwindcss >=4` |
| shadcn/ui | copy-the-source + Tailwind; nothing to import at runtime |
| Chakra v3 | heavier styled-system runtime for no gain here |

Three survive — Radix Themes, Mantine, MUI — all shipping importable CSS. Radix
wins on the thing this app actually needs: **theming is four props on a wrapper**,
so a theme change is a settings write, not a regeneration. Mantine's theme is a JS
object and MUI's is a provider tree; both work, both need more plumbing to change
live.

Its smaller component count (~50 vs Astryx's 150) is a feature here. The contract
lists them exhaustively; a list a model can hold is a list it will not hallucinate.

## Reset truncates; only compaction rewrites

These were conflated once and it is worth keeping straight, because the mistake
looks reasonable: both delete versions, so both feel like they should invalidate
the hash chain.

- **Reset removes a suffix.** No surviving hash folded in a row that went, so the
  survivors keep their exact hashes and the chain still verifies untouched. `git
  reset --hard`. The head value changes only because the tip changed.
- **Compaction removes rows from the middle.** The row after a deleted one folded
  its hash in, so it must be re-stamped — and re-stamping is what destroys
  tamper-evidence for everything before. `git filter-branch`.

`reset_to` therefore does **not** touch hashes, and verifies afterwards instead:
if a truncation ever leaves the chain broken, that is a bug to surface, not to
paper over by recomputing until the numbers agree with the disk. Only
`compact_history` re-stamps, and only in a debug build.

`db::truncation_tests` pins all three claims, including the contrast case.

## The identifier and the feed host are frozen

`com.claya.app` and `https://updates.claya.app/...` are both compiled into every
signed binary, and neither can be changed afterwards without consequences that
cannot be patched from our side.

- **Changing the identifier** makes macOS treat the result as a *different app*:
  new Application Support directory, separate keychain items, separate
  LaunchServices registration. Existing installs never auto-update into it, and
  every user's module history is stranded at the old path.
- **Changing the feed host** orphans the update channel. Installed clients only
  ever ask the old host, so whoever holds that domain next controls their updates.

They were set during the `self-build` → `claya` rename, before anything shipped,
which is the only moment either was free. `com.claya.app` is reverse-DNS of the
domain that serves the feed, and both live on `claya.app` rather than `claya.dev`
so that exactly one domain is unlosable rather than two.

If a future rename is ever genuinely required, it is not a find-and-replace — it
needs a migration that moves the data directory and keychain entries, and a final
update pushed from the *old* feed telling clients where to look next.
