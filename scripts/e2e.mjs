/**
 * End-to-end check: boots the real app against a throwaway database, applies a
 * module that cannot compile, and asserts the gate rolled it back.
 *
 * Uses a temp DB so it never touches your real history — the earlier version of
 * this check left six junk versions behind.
 */
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const dir = mkdtempSync(join(tmpdir(), "claya-e2e-"));
const db = join(dir, "e2e.db");
const deadline = 180_000;

const child = spawn("npm", ["run", "tauri", "dev"], {
  env: {
    ...process.env,
    CLAYA_DB: db,
    CLAYA_VERIFY_ROLLBACK: "1",
    CLAYA_VERIFY_TEMPLATES: "1",
    CLAYA_EXIT_AFTER_TEST: "1",
  },
  stdio: ["ignore", "pipe", "pipe"],
});

const checks = [
  { name: "canvas mounts a runtime-compiled module", re: /\[self-test\].*"mounted":true/ },
  { name: "capability isolation enforced", re: /\[self-test\].*"isolated":true/ },
  { name: "history integrity verifies", re: /\[integrity\] ok=true/ },
  { name: "gate classifies the failure", re: /\[gate-test\].*phase=Some\("transpile"\)/ },
  { name: "gate rolls back to the last good version", re: /\[gate-test\].*restored=true/ },
  { name: "canvas recovers after rollback", re: /\[gate-test\] after rollback.*"ok":true/ },
  { name: "template: todo mounts through the gate", re: /\[template-test\] todo applied=true/ },
  { name: "template: notes mounts through the gate", re: /\[template-test\] notes applied=true/ },
  { name: "template: tracker mounts through the gate", re: /\[template-test\] tracker applied=true/ },
  { name: "template: dashboard mounts through the gate", re: /\[template-test\] dashboard applied=true/ },
  { name: "samples seeded into an empty store only, never again", re: /\[template-test\] keys carrying sample rows: 1$/m },
];

let output = "";
const collect = (chunk) => {
  output += chunk;
  process.stdout.write(chunk.toString().split("\n").filter((l) => /\[(self-test|gate-test|template-test|integrity|db)\]/.test(l)).map((l) => `  ${l}\n`).join(""));
};
child.stdout.on("data", collect);
child.stderr.on("data", collect);

const timer = setTimeout(() => {
  console.error("\ne2e timed out");
  child.kill("SIGKILL");
}, deadline);

child.on("exit", () => {
  clearTimeout(timer);
  rmSync(dir, { recursive: true, force: true });

  console.log("");
  let failed = 0;
  for (const { name, re } of checks) {
    const ok = re.test(output);
    if (!ok) failed++;
    console.log(`${ok ? "PASS" : "FAIL"}  ${name}`);
  }
  console.log(`\n${checks.length - failed}/${checks.length} end-to-end checks passed`);
  process.exit(failed === 0 ? 0 : 1);
});
