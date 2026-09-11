# Review of the original plan

The original plan (`00-original-plan.md`) had the right stack and the right shape.
Four things in it would not have survived contact with a release build.

## 1. The production problem was a compiler problem, not a path problem

Phase 4.1 treated "compiled Tauri binaries embed frontend assets" as something you
fix by pointing the webview at a writable directory. But the model writes `.tsx`.
A shipped app has no Vite, no TypeScript, no bundler — writing TSX to a writable
directory produces a file nothing can execute.

Four ways out were considered:

| | Approach | Verdict |
|---|---|---|
| A | Runtime transpile (Sucrase in-process) | **Chosen.** ~1 MB, needs `unsafe-eval` |
| B | Ship Node + Vite as a sidecar | +150 MB, slow, notarization pain |
| C | LLM emits a JSON UI spec, fixed renderer | Safest, capped at what the schema expresses |
| D | Dev-only (`tauri dev`) | Fine for a demo, never ships |

## 2. The guardrail did not guard

```rust
if !target_path.starts_with("src/") { return Err(...) }
```

`Path::starts_with` compares *components*, so `src/../../../etc/passwd` begins with
`src` and passes — then `fs::write` resolves the `..`. Separately the path is
relative, and a bundled `.app` runs with CWD `/`, so it would have written to
`/src/`. Moot in the end: modules live in SQLite, not on disk.

## 3. The API key was in the frontend

`Bearer YOUR_KEY` in `ChatPanel.tsx` ships the key in a bundle any user can unzip —
and worse, in the same directory the model is editing. Moved to Rust + Keychain.

## 4. One window meant no way back

With chat and app in one webview, the first module that fails to render takes the
chat panel with it, and you cannot ask it to undo. Two windows with different
capability grants fixes this and doubles as the security boundary.

## What replaced it

- **Two windows.** `shell` is hand-written and privileged; `canvas` runs
  model-authored code and is granted almost nothing. Enforced by Tauri 2
  capabilities, which scope per window label.
- **Modules in SQLite, not on disk.** Append-only with a live pointer, so a swap is
  a transaction and rollback is a pointer move.
- **Verification driven from Rust.** `eval_with_callback` lets the privileged side
  run the health probe; the module under test never votes on its own health.
