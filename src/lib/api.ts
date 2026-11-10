// One API for both front ends: Tauri IPC inside the desktop popup, HTTP + SSE
// when the same UI is served by the built-in web server.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { EffectName, EffectOptions, Patch, SceneInput, SettingsUpdate, SettingsView, Snapshot } from "./types";

export interface Api {
  desktop: boolean;
  snapshot(): Promise<Snapshot>;
  subscribe(onSnapshot: (s: Snapshot) => void, onConnection?: (ok: boolean) => void): () => void;
  command(targets: string[], patch: Patch): Promise<void>;
  refresh(): Promise<void>;
  startEffect(name: EffectName, targets: string[], options?: EffectOptions): Promise<void>;
  stopEffects(targets: string[]): Promise<void>;
  /** Accept the one-time "effects are experimental" warning. */
  acknowledgeEffects(): Promise<void>;
  applyScene(name: string, targets: string[]): Promise<void>;
  saveScene(name: string, scene: SceneInput): Promise<void>;
  deleteScene(name: string): Promise<void>;
  getSettings(): Promise<SettingsView>;
  saveSettings(update: SettingsUpdate): Promise<SettingsView>;
  /** Desktop events: "navigate" (payload: view name) and "popup-shown". */
  on(event: "navigate" | "popup-shown", handler: (payload: unknown) => void): () => void;
  hide(): void;
  openWeb(): void;
  quit(): void;
}

function tauriListen<T>(event: string, handler: (payload: T) => void): () => void {
  let unlisten: (() => void) | undefined;
  let disposed = false;
  listen<T>(event, (e) => handler(e.payload)).then((fn) => (disposed ? fn() : (unlisten = fn)));
  return () => {
    disposed = true;
    unlisten?.();
  };
}

function tauriApi(): Api {
  return {
    desktop: true,
    snapshot: () => invoke<Snapshot>("snapshot"),
    subscribe: (onSnapshot) => tauriListen<Snapshot>("snapshot", onSnapshot),
    command: (targets, patch) => invoke("command", { targets, patch }),
    refresh: () => invoke("refresh"),
    startEffect: (name, targets, options) => invoke("start_effect", { name, targets, options: options ?? null }),
    stopEffects: (targets) => invoke("stop_effects", { targets }),
    acknowledgeEffects: () => invoke("acknowledge_effects"),
    applyScene: (name, targets) => invoke("apply_scene", { name, targets }),
    saveScene: (name, scene) => invoke("save_scene", { name, scene }),
    deleteScene: (name) => invoke("delete_scene", { name }),
    getSettings: () => invoke<SettingsView>("get_settings"),
    saveSettings: (update) => invoke<SettingsView>("save_settings", { update }),
    on: (event, handler) => tauriListen(event, handler),
    hide: () => void invoke("hide_window"),
    openWeb: () => void invoke("open_web"),
    quit: () => void invoke("quit_app"),
  };
}

async function http<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(path, {
    method,
    headers: body === undefined ? undefined : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) throw new Error((await res.text()) || `${res.status} ${res.statusText}`);
  return (res.status === 204 ? undefined : await res.json()) as T;
}

function httpApi(): Api {
  return {
    desktop: false,
    snapshot: () => http<Snapshot>("GET", "/api/snapshot"),
    subscribe(onSnapshot, onConnection) {
      let source: EventSource | undefined;
      let closed = false;
      let retry: number | undefined;
      const open = () => {
        source = new EventSource("/api/events");
        source.addEventListener("snapshot", (e) => onSnapshot(JSON.parse((e as MessageEvent<string>).data)));
        source.onopen = () => onConnection?.(true);
        source.onerror = () => {
          onConnection?.(false);
          source?.close();
          if (!closed) retry = window.setTimeout(open, 2000);
        };
      };
      open();
      return () => {
        closed = true;
        clearTimeout(retry);
        source?.close();
      };
    },
    command: (targets, patch) => http("POST", "/api/command", { targets, patch }),
    refresh: () => http("POST", "/api/refresh"),
    startEffect: (name, targets, options) => http("POST", "/api/effects/start", { name, targets, options: options ?? {} }),
    stopEffects: (targets) => http("POST", "/api/effects/stop", { targets }),
    acknowledgeEffects: () => http("POST", "/api/effects/acknowledge"),
    applyScene: (name, targets) => http("POST", "/api/scenes/apply", { name, targets }),
    saveScene: (name, scene) => http("POST", "/api/scenes", { name, scene }),
    deleteScene: (name) => http("DELETE", `/api/scenes/${encodeURIComponent(name)}`),
    getSettings: () => http<SettingsView>("GET", "/api/settings"),
    saveSettings: (update) => http<SettingsView>("PUT", "/api/settings", update),
    on: () => () => {},
    hide: () => {},
    openWeb: () => {},
    quit: () => {},
  };
}

export const api: Api = "__TAURI_INTERNALS__" in window ? tauriApi() : httpApi();

/** Call at most every `ms`, always delivering the latest value last. */
export function throttle<T>(fn: (value: T) => void, ms: number): (value: T) => void {
  let last = 0;
  let timer: number | undefined;
  let latest: T;
  return (value: T) => {
    latest = value;
    const wait = last + ms - performance.now();
    if (wait <= 0) {
      last = performance.now();
      fn(value);
    } else if (timer === undefined) {
      timer = window.setTimeout(() => {
        timer = undefined;
        last = performance.now();
        fn(latest);
      }, wait);
    }
  };
}

export function errorText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
