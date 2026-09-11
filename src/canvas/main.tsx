import React, { Component, StrictMode, useEffect, useState, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import * as UI from "@radix-ui/themes";
import { compileModule, type LoadResult } from "./loader";
import { host } from "./host";
import "@radix-ui/themes/styles.css";
import "./canvas.css";

/** Everything a model-authored module is allowed to import. */
const REGISTRY: Record<string, unknown> = {
  react: React,
  "@host": { host },
  "@ui": UI,
};

interface CanvasTheme {
  accentColor: string;
  grayColor: string;
  radius: string;
  scaling: string;
  appearance: string;
}

declare global {
  interface Window {
    __set_theme?: (t: CanvasTheme) => void;
  }
}

/**
 * The theme lives OUTSIDE the module, wrapping it.
 *
 * That separation is the point: a module owns structure and components, the
 * theme owns how they look. Changing the theme therefore re-skins every version
 * ever saved — including modules written before the theme existed — with no
 * regeneration and no model call.
 *
 * Rust pushes updates in through `window.__set_theme` rather than an event,
 * because the canvas is deliberately not granted event access.
 */
function Themed({ initial, children }: { initial: CanvasTheme; children: ReactNode }) {
  const [theme, setTheme] = useState(initial);
  useEffect(() => {
    window.__set_theme = setTheme;
    return () => {
      delete window.__set_theme;
    };
  }, []);
  return (
    <UI.Theme
      accentColor={theme.accentColor as never}
      grayColor={theme.grayColor as never}
      radius={theme.radius as never}
      scaling={theme.scaling as never}
      appearance={theme.appearance as never}
    >
      {children}
    </UI.Theme>
  );
}

interface ProbeReport {
  ok: boolean;
  /**
   * Identifies THIS page load. Straight after a reload the outgoing document is
   * briefly still alive and still answering — without a generation token the
   * verifier can read the old page's verdict and commit a module it never
   * actually tested.
   */
  boot: string;
  isolated: boolean | null;
  phase?: string;
  error?: string;
  mounted: boolean;
  nodeCount: number;
  at: number;
}

const BOOT_ID = Math.random().toString(36).slice(2, 10);
let lastResult: LoadResult | null = null;
let mounted = false;

/**
 * Self-test: this window must NOT reach a shell-only command.
 *
 * `list_versions` is the canary rather than `save_module` because it is
 * read-only — if isolation were ever broken, probing the write path would
 * itself corrupt the store it was meant to be checking.
 */
let isolated: boolean | null = null;
invoke("list_versions", { name: "app" }).then(
  () => {
    isolated = false;
    console.error("[canvas] ISOLATION BREACH: shell-only command succeeded");
  },
  () => {
    isolated = true;
  }
);

/**
 * Read by Rust via `eval_with_callback`. The privileged side decides whether a
 * swap committed — the module under test never gets to vote on its own health.
 */
declare global {
  interface Window {
    __canvas_probe: () => ProbeReport;
  }
}

window.__canvas_probe = () => ({
  ok: mounted && lastResult?.ok === true,
  boot: BOOT_ID,
  isolated,
  phase: lastResult && !lastResult.ok ? lastResult.phase : undefined,
  error: lastResult && !lastResult.ok ? lastResult.error : undefined,
  mounted,
  nodeCount: document.querySelectorAll("#root *").length,
  at: Date.now(),
});

class Boundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) {
    return { error };
  }
  componentDidCatch(error: Error) {
    mounted = false;
    lastResult = { ok: false, phase: "evaluate", error: error.message };
  }
  render() {
    if (this.state.error) {
      return <Failure phase="render" message={this.state.error.message} />;
    }
    return this.props.children;
  }
}

function Failure({ phase, message }: { phase: string; message: string }) {
  return (
    <div className="failure">
      <h2>Module failed at {phase}</h2>
      <pre>{message}</pre>
      <p>The shell window is unaffected — revert or fix from there.</p>
    </div>
  );
}

let root: Root | null = null;
function paint(node: ReactNode) {
  root ??= createRoot(document.getElementById("root")!);
  root.render(node);
}

async function boot(source: string, theme: CanvasTheme) {
  const result = compileModule(source, REGISTRY);
  lastResult = result;

  if (!result.ok) {
    mounted = false;
    paint(<Failure phase={result.phase} message={result.error} />);
    return;
  }

  // Registered BEFORE the first render, because the module writes on mount and
  // an unregistered schema means that first write replaces instead of grafting.
  if (result.schema) {
    try {
      await invoke("declare_schema", { schema: result.schema });
    } catch (e) {
      console.error("[canvas] schema rejected — data is unprotected", e);
    }
  }

  const Loaded = result.component as unknown as React.ComponentType;
  mounted = true;
  paint(
    <StrictMode>
      <Themed initial={theme}>
        <Boundary>
          <Loaded />
        </Boundary>
      </Themed>
    </StrictMode>
  );
}

async function main() {
  try {
    const [source, theme] = await Promise.all([
      invoke<string>("load_active_module", { name: "app" }),
      invoke<CanvasTheme>("get_theme"),
    ]);
    await boot(source, theme);
  } catch (e) {
    mounted = false;
    lastResult = { ok: false, phase: "contract", error: String(e) };
    paint(<Failure phase="load" message={String(e)} />);
  }
}

void main();
