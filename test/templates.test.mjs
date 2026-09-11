/**
 * Every shipped template must survive the exact path a model-authored module
 * takes: import check, Sucrase, `new Function`, and the default-export contract.
 * These are the first thing a new user sees — one that fails the gate on a fresh
 * install is unrecoverable from inside the app.
 */
import { compileModule } from "../.loader-build/loader.mjs";
import React from "react";
import * as UI from "@radix-ui/themes";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const dir = "src/canvas/seed-modules";
// Must mirror REGISTRY in src/canvas/main.tsx exactly — a test registry that
// drifts from the real one proves nothing about what will actually load.
const registry = {
  react: React,
  "@host": { host: { state: { get: async (_k, f) => f, set: async () => {} }, log() {} } },
  "@ui": UI,
};

const files = readdirSync(dir).filter((f) => f.endsWith(".seed.tsx")).sort();
let failed = 0;

for (const file of files) {
  const source = readFileSync(join(dir, file), "utf8");
  const r = compileModule(source, registry);

  if (!r.ok) {
    failed++;
    console.log(`FAIL  ${file.padEnd(20)} ${r.phase} — ${r.error.split("\n")[0].slice(0, 70)}`);
    continue;
  }
  // A template with no schema leaves its data unprotected from the very first
  // write, which defeats M6 for exactly the users who need it most.
  if (!r.schema || Object.keys(r.schema).length === 0) {
    failed++;
    console.log(`FAIL  ${file.padEnd(20)} compiled but declares no schema`);
    continue;
  }
  const keys = Object.keys(r.schema).join(", ");
  console.log(`PASS  ${file.padEnd(20)} owns: ${keys}`);
}

console.log(`\n${files.length - failed}/${files.length} templates ready`);
process.exit(failed === 0 ? 0 : 1);
