# Principles

What this app is trying to be, and what that rules out. Each one names the
mechanism that enforces it, because a principle nothing checks is decoration.

## Nothing the user does *by accident* is unrecoverable

**Any state the user has been in, they can get back to — with the data they had
at the time — unless they explicitly asked to destroy it.** An app that rewrites
itself is only usable if a bad rewrite costs nothing, and "costs nothing" has to
include the user's data, not just the code.

The exception is deliberate and named: **Reset** (below). Everything else — every
edit, every switch, every gate rollback — is reversible.

Enforced by:

- **Every edit is an `INSERT`.** `active_module` is a pointer at whichever version
  is live, so a swap is a transaction and a rollback is a pointer move. No version
  is ever destroyed — see `db.rs`.
- **`parent_version` records what a version branched from.** Roll back to v1, edit,
  and you get a v3 whose parent is v1. The lineage survives rather than the history
  reading as a straight line that never happened.
- **Every version change snapshots `kv` at the boundary.** Bounded by switches
  rather than by writes, so the cost is proportional to how often the app changes
  shape, not to how much the user types.
- **The gate rolls back automatically.** A version that fails its probe never stays
  live; `src-tauri/src/lib.rs` reverts and keeps the failure in the history so it
  can be read.

## The escape hatch cannot be captured by the thing it escapes

The control you use to undo a bad generation must never be something a bad
generation can hide, move, or style into invisibility.

Enforced by: **the drawer toggle lives in the tray**, not in the canvas. The
canvas is written by the model; the tray is not reachable from it.

## The model's code is untrusted, and that is enforced in Rust

Not by convention, not by review, not by prompt.

Enforced by: **capabilities are scoped per window**, and `src-tauri/build.rs`
registers commands into the ACL via `AppManifest::commands()`. Without that step
Tauri makes every command callable from every window and the isolation is
decorative — nothing warns you. The canvas proves the boundary at boot by
attempting a privileged call and expecting rejection.

The canvas may **read** its own source. It may never **write** a version.

## The thing under test does not get a vote on its own health

A module that has wedged cannot be trusted to report that it has wedged.

Enforced by: the probe runs from the privileged side via `eval_with_callback`,
and **the timeout is an answer**. Wedged code simply fails to reply. The probe
report carries a `boot` token so a dying module cannot answer on the next one's
behalf.

## Failure has to be legible to whoever caused it

Enforced by: the loader reports the **phase** — `imports` / `transpile` /
`evaluate` / `contract` / `render`. "It broke" tells the model nothing; "you
imported something that isn't in the registry" tells it exactly what to fix.

## Data flows forward, and the cost of that is paid by snapshots

App data is **shared across versions, not namespaced per version**. Namespacing
would prevent every conflict and also make the user's data vanish each time the
app changed shape — which is the opposite of the first principle.

The cost is real: an older module can strip a field it does not know about. That
is bought back with boundary snapshots and with schema declarations, which graft
writes rather than replacing wholesale.

---

## What this rules out

Decisions that would violate the above, so they are not proposed again:

- **Hard-deleting a version, a snapshot, or the history** — *in anything a user
  runs*. Anything offering to "clean up" old versions is removing the user's
  ability to get back.

  **There is exactly one way to delete: Reset (below).** An earlier version of
  this app also offered dev-only *compaction* — removing failed versions from the
  middle of the history — and it was taken out on purpose. Middle-removal forces
  the rows after the deleted one to be re-hashed, because each hash folds in its
  predecessor, and re-hashing is what destroys tamper-evidence.

  Removing it buys an unconditional guarantee: **no operation this app offers
  ever overwrites a hash it has already written.** A version's hash is written
  once, when the version is created, and from then on it is either that exact
  value or the row is gone. So a hash recorded at any point in the past still
  verifies today, and "this history has not been edited" is a plain statement
  rather than one qualified by "…since the last compaction".

  Pinned by `db::immutability_tests`, which runs every mutating operation in the
  app and asserts no surviving hash moved.

  In production the answer to a *cluttered* list is **hiding**: the Versions panel
  filters failed versions and nothing is lost.

## Switch and Reset are different verbs, and the difference is the whole point

- **Switch to vN** moves the live pointer. History is untouched, data is
  untouched, and you can switch straight back. Reversible, and the default.
- **Reset to vN** truncates: every version after N is *deleted*, app data is
  cleared, and the next edit is vN+1. This is what keeps the history a clean
  line instead of a growing pile of abandoned branches — resetting to v1 and
  editing must give v2, not v11.

Reset is destructive on purpose and it is the one place the first principle
yields. Three things still hold:

- **It is explicit.** A button per row, behind a confirm that names exactly how
  many versions will go — and the count is re-checked against the chain head at
  the moment it fires. If the history moved since you looked, the reset is refused
  rather than silently deleting more than the confirm promised.
- **It snapshots the data first**, so a reset fired by mistake is recoverable
  from the snapshots panel. The versions are not.
- **It does NOT rewrite the integrity chain, and must not.** Truncation removes a
  suffix, so no surviving hash depends on a row that went — `h1` stays `h1`
  however many later versions are deleted. This is `git reset --hard`: the
  commits that remain keep their SHAs, only HEAD moves. Tamper-evidence over what
  survives is fully intact, and a hash recorded before a reset still verifies
  after it.

  The head value does change, because the head *is* the tip and the tip moved.
  That is a different statement from "the history was rewritten".

  **The operation that genuinely destroys tamper-evidence is dev-only
  compaction**, which removes rows from the *middle*: the row after a deleted one
  folded its hash in, so it has to be re-stamped. That is a rebase, not a reset,
  and the distinction is why one ships and the other does not.
- **A destructive action that does not snapshot first.** Including — especially —
  the ones that exist *in order* to be destructive, like "reset to". The reset
  itself has to be reversible.
- **Moving the escape hatch into model-authored surface.** Any control.
- **Granting the canvas a write command** for convenience, however narrow.
- **Trusting a self-report from the canvas** as evidence of health.
- **Namespacing app data per version** to dodge the field-stripping problem. It
  trades a recoverable cost for an unrecoverable one.
