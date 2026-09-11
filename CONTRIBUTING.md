# Contributing to Claya

Claya is a macOS app that rewrites its own interface from a chat prompt. The
model writes the app; the app decides whether to keep what it wrote.

## Getting it running

```bash
npm install
export ANTHROPIC_API_KEY=sk-ant-...   # optional; avoids a Keychain prompt per rebuild
npm run tauri dev
```

Rust is expected at `~/.cargo/bin`; the npm scripts put it on `PATH`
themselves, so nothing needs to be in your shell profile.

## Tests

```bash
npm test          # loader + templates + Rust. Run after every change; sub-second.
npm run test:e2e  # boots the real app against a throwaway database
```

`npm test` is fast and offline. `test:e2e` starts the actual binary, applies a
module that cannot compile, and asserts the gate rolled it back — it is the only
check that exercises the real windows and the real capability boundary.

Neither suite calls the Claude API. To drive a real turn by hand:

```bash
CLAYA_PROMPT="make the background black" npm run tauri dev
```

That costs money, so it is not in CI.

## Before you change anything, read PRINCIPLES.md

It is short, and it names the mechanism enforcing each rule. Three that are
easy to break without noticing:

- **`src-tauri/build.rs` is load-bearing.** `AppManifest::commands()` is what
  puts app commands behind the ACL. Remove it and Tauri makes every command
  callable from every window — including the one running model-authored code —
  and nothing warns you. The canvas proves the boundary at boot by attempting a
  privileged call and expecting rejection.
- **The canvas may read its own source. It may never write a version.** If you
  find yourself granting it a write command for convenience, that is the bug.
- **The module contract in `src-tauri/src/llm.rs` mirrors the registry in
  `src/canvas/main.tsx`.** Widen one without the other and the model will write
  imports that are rejected at runtime. Same commit, always.

## Where things live

```
src/shell/        hand-written, privileged. The chat drawer. Never model-authored.
src/canvas/       runs model-authored code
  loader.ts       Sucrase + new Function — the runtime compiler
  host.ts         the API model-authored code may use
  seed-modules/   the shipped templates
src-tauri/src/
  db.rs           versions, snapshots, schema grafting, the integrity chain
  llm.rs          the Claude call and the module contract
  lib.rs          commands, the verify-then-commit gate
plans/            why things are the way they are. 03-decisions.md especially.
```

## Sign your commits off (DCO)

Claya uses the [Developer Certificate of Origin](https://developercertificate.org/)
rather than a CLA. You keep copyright in your contribution; you are certifying
you have the right to submit it under Apache-2.0.

```bash
git commit -s -m "your message"
```

`-s` appends `Signed-off-by: Your Name <your@email>`. Commits without it will be
asked to amend.

## Pull requests

- One concern per PR.
- `npm test` green, and `npm run test:e2e` if you touched the gate, the loader,
  the capability files, or anything under `src-tauri/`.
- New behaviour comes with a test. The bar is not coverage — it is that the test
  would have failed before your change.
- If you fixed something subtle, add it to `plans/04-bugs-found.md`. That file
  exists because most of the real bugs in this project were invisible until one
  specific thing was tried.
