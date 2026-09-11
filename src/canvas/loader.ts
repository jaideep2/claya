import { transform } from "sucrase";

export type LoadPhase = "imports" | "transpile" | "evaluate" | "contract";

export interface LoadOk {
  ok: true;
  component: (props: Record<string, unknown>) => unknown;
  /** The module's `schema` export, if it declared one. Registered with Rust
   *  before mount so writes are grafted rather than replaced. */
  schema: Record<string, unknown> | null;
}
export interface LoadErr {
  ok: false;
  phase: LoadPhase;
  error: string;
}
export type LoadResult = LoadOk | LoadErr;

/**
 * Compile a TSX source string into a live React component, at runtime.
 *
 * Two stages, reported separately so the shell can tell the model whether it
 * wrote code that doesn't parse or code that doesn't run:
 *
 *   1. Sucrase strips TypeScript + JSX and rewrites ESM imports to CJS-style
 *      `require` calls. No bundler, no network, ~1ms for a file this size.
 *   2. The result is evaluated via `new Function`, with `require` bound to a
 *      fixed registry. A module can only reach what we hand it.
 */
export function compileModule(source: string, registry: Record<string, unknown>): LoadResult {
  // Checked BEFORE transpiling, because Sucrase elides imports whose bindings
  // are unused — it cannot tell them from type-only imports. Without this, a
  // forbidden import is silently dropped and only explodes later, at a point
  // that tells the model nothing useful.
  const importProblem = checkImports(source, registry);
  if (importProblem) return { ok: false, phase: "imports", error: importProblem };

  let code: string;
  try {
    ({ code } = transform(source, {
      transforms: ["typescript", "jsx", "imports"],
      jsxRuntime: "classic",
      jsxPragma: "React.createElement",
      jsxFragmentPragma: "React.Fragment",
      production: true,
    }));
  } catch (e) {
    return { ok: false, phase: "transpile", error: describe(e) };
  }

  try {
    const require = (specifier: string) => {
      if (!(specifier in registry)) {
        throw new Error(
          `Module "${specifier}" is not available. You may import: ${Object.keys(registry).join(", ")}`
        );
      }
      return registry[specifier];
    };

    const module = { exports: {} as Record<string, unknown> };
    const factory = new Function(
      "require",
      "module",
      "exports",
      "React",
      `"use strict";\n${code}`
    );
    factory(require, module, module.exports, registry.react);

    const component = module.exports.default ?? module.exports.Component;
    if (typeof component !== "function") {
      return {
        ok: false,
        phase: "contract",
        error: "Module must default-export a React component function.",
      };
    }
    const schema = module.exports.schema;
    return {
      ok: true,
      component: component as LoadOk["component"],
      schema: (schema && typeof schema === "object" ? schema : null) as LoadOk["schema"],
    };
  } catch (e) {
    return { ok: false, phase: "evaluate", error: describe(e) };
  }
}

const SPECIFIER_RE =
  /\bfrom\s*["\']([^"\']+)["\']|^\s*import\s*["\']([^"\']+)["\']|\brequire\s*\(\s*["\']([^"\']+)["\']\s*\)/gm;

/**
 * Reject unknown module specifiers up front, with a message naming what IS
 * available — that string goes straight back to the model in milestone 4.
 *
 * Deliberately a scan, not a parse: it can false-positive on `from "x"` inside
 * a string literal. A spurious, actionable error beats a silent hole.
 */
function checkImports(source: string, registry: Record<string, unknown>): string | null {
  if (/\bimport\s*\(/.test(source)) {
    return "Dynamic import() is not allowed — it bypasses the module registry.";
  }
  const unknown = new Set<string>();
  for (const match of source.matchAll(SPECIFIER_RE)) {
    const spec = match[1] ?? match[2] ?? match[3];
    if (spec && !(spec in registry)) unknown.add(spec);
  }
  if (unknown.size === 0) return null;
  const listed = [...unknown].map((s) => `"${s}"`).join(", ");
  return `Cannot import ${listed}. You may import: ${Object.keys(registry).join(", ")}.`;
}

function describe(e: unknown): string {
  if (e instanceof Error) return e.stack ? `${e.message}\n\n${e.stack}` : e.message;
  return String(e);
}
