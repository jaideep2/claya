import { invoke } from "@tauri-apps/api/core";

/**
 * The API surface exposed to model-authored modules.
 *
 * This is the ONLY way canvas code touches persistence. It now goes through
 * Rust into SQLite — the canvas has `kv_get`/`kv_set` and nothing else, so the
 * blast radius of a bad module is its own key namespace.
 */
export interface Host {
  state: {
    get<T>(key: string, fallback: T): Promise<T>;
    set(key: string, value: unknown): Promise<void>;
  };
  log(...args: unknown[]): void;
}

export const host: Host = {
  state: {
    async get<T>(key: string, fallback: T): Promise<T> {
      try {
        const raw = await invoke<string | null>("kv_get", { key });
        return raw === null ? fallback : (JSON.parse(raw) as T);
      } catch (e) {
        console.error("[host] kv_get failed", key, e);
        return fallback;
      }
    },
    async set(key: string, value: unknown): Promise<void> {
      await invoke("kv_set", { key, value: JSON.stringify(value) });
    },
  },
  log(...args: unknown[]) {
    console.log("[canvas]", ...args);
  },
};
