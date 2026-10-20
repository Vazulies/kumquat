import { invoke } from "@tauri-apps/api/core";

export type WheelKind = "convert" | "tools" | "actions";
export type Modifier = "Shift" | "Control" | "Alt" | "Super" | "Space";

export interface Item {
  id: string;
  label: string;
  hint: string;
  icon: string;
  editor: string | null;
}

export interface Catalog {
  kind: string | null;
  count: number;
  title: string;
  convert: Item[];
  tools: Item[];
  actions: Item[];
}

export interface KeyHints {
  convert: Modifier;
  tools: Modifier;
  actions: Modifier;
}

export interface Settings {
  convertKey: Modifier;
  toolsKey: Modifier;
  actionsKey: Modifier;
  outputBesideOriginal: boolean;
  outputFolder: string | null;
  imageQuality: number;
  videoCrf: number;
  pdfDpi: number;
  revealAfter: boolean;
  notifications: boolean;
  paused: boolean;
  onboarded: boolean;
}

export interface ToolStatus {
  id: string;
  purpose: string;
  path: string | null;
  bundled: boolean;
}

export interface EditorJob {
  item: Item;
  paths: string[];
}

export interface Probe {
  duration: number | null;
  width: number | null;
  height: number | null;
  videoCodec: string | null;
  audioCodec: string | null;
}

export const api = {
  catalog: (paths: string[]) => invoke<Catalog>("get_catalog", { paths }),
  runItem: (id: string, paths: string[], params?: unknown) =>
    invoke<void>("run_item", { id, paths, params: params ?? null }),
  closeWheel: () => invoke<void>("wheel_close"),
  repaintWheel: () => invoke<void>("wheel_repaint"),
  settings: () => invoke<Settings>("get_settings"),
  saveSettings: (value: Settings) => invoke<Settings>("save_settings", { value }),
  toolStatus: () => invoke<ToolStatus[]>("tool_status"),
  integrationStatus: () =>
    invoke<{ installed: boolean; managers: string[]; shortcut: string; hint: string }>("integration_status"),
  setIntegration: (on: boolean) =>
    invoke<{ installed: boolean; managers: string[]; shortcut: string; hint: string }>("set_integration", { on }),
  autostart: () => invoke<boolean>("get_autostart"),
  setAutostart: (on: boolean) => invoke<boolean>("set_autostart", { on }),
  session: () =>
    invoke<{ sessionType: string; platform: string; fileManager: string; dragWheel: boolean }>("session_info"),
  editorContext: () => invoke<EditorJob | null>("editor_context"),
  editorSubmit: (params: unknown) => invoke<void>("editor_submit", { params }),
  editorCancel: () => invoke<void>("editor_cancel"),
  imagePreview: (path: string, max: number) =>
    invoke<{ src: string; width: number; height: number }>("image_preview", { path, max }),
  mediaInfo: (path: string) => invoke<Probe>("media_info", { path }),
  mediaFrame: (path: string, at: number, width: number) =>
    invoke<string>("media_frame", { path, at, width }),
  mediaWaveform: (path: string, buckets: number) =>
    invoke<number[]>("media_waveform", { path, buckets }),
  pdfInfo: (path: string) => invoke<number>("pdf_info", { path }),
  pdfPage: (path: string, index: number, width: number) =>
    invoke<string>("pdf_page", { path, index, width }),
  floatContext: () =>
    invoke<{ paths: string[]; icon: string; preview: string | null } | null>("float_context"),
  floatClose: () => invoke<void>("float_close"),
};

// Report uncaught frontend errors to the app log.
const report = (message: string) => invoke("js_log", { message }).catch(() => {});
window.addEventListener("error", (e) => report(`${e.message} at ${e.filename}:${e.lineno}`));
window.addEventListener("unhandledrejection", (e) => report(`unhandled: ${e.reason}`));

/** Short display labels for modifier keys. */
export const KEY_LABEL: Record<Modifier, string> = {
  Shift: "⇧ Shift",
  Control: "Ctrl",
  Alt: "Alt",
  Space: "Space",
  Super: navigator.userAgent.includes("Windows") ? "Win" : "Super",
};

export function fileName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

export function extOf(path: string): string {
  const name = fileName(path).toLowerCase();
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(dot + 1) : "";
}

export function formatTime(seconds: number): string {
  const s = Math.max(0, seconds);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = (s % 60).toFixed(1).padStart(4, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${sec}` : `${m}:${sec}`;
}

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<HTMLElementTagNameMap[K]> & { class?: string } = {},
  ...children: (Node | string | null | undefined)[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  const { class: cls, ...rest } = props as { class?: string };
  if (cls) node.className = cls;
  Object.assign(node, rest);
  for (const child of children) {
    if (child == null) continue;
    node.append(child);
  }
  return node;
}
