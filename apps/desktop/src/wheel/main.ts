import "./wheel.css";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, Catalog, Item, KeyHints, Modifier, WheelKind } from "../shared/api";
import { icon } from "../shared/icons";

const SIZE = 560;
const C = SIZE / 2;
const R_HUB = 56;
/** Every tile is this wide (at mid-ring) and this deep, whatever the count. */
const TILE_WIDTH = 104;
const TILE_DEPTH = 104;
/** The ring never gets smaller than this (mid-ring radius). */
const MIN_MID = 116;

// Ring geometry for the current wheel: more tiles push the ring outward
// instead of squeezing the tiles.
let R_IN = MIN_MID - TILE_DEPTH / 2;
let R_OUT = MIN_MID + TILE_DEPTH / 2;
let R_LABEL = MIN_MID + 2;

function sizeRing(n: number) {
  const mid = Math.max(MIN_MID, (n * TILE_WIDTH) / (Math.PI * 2));
  R_IN = mid - TILE_DEPTH / 2;
  R_OUT = mid + TILE_DEPTH / 2;
  R_LABEL = mid + 2;
}
const GAP = 6;
const KINDS: WheelKind[] = ["convert", "tools", "actions"];
const KIND_TITLE: Record<WheelKind, string> = { convert: "Convert", tools: "Tools", actions: "Actions" };
const KEY_SHORT: Record<Modifier, string> = { Shift: "⇧", Control: "Ctrl", Alt: "Alt", Super: "❖", Space: "Space" };
const SVG = "http://www.w3.org/2000/svg";

interface OpenPayload {
  kind: WheelKind;
  mode: "drag" | "keyboard";
  paths: string[];
  catalog: Catalog | null;
  keys: KeyHints;
  backdrop: string | null;
}

const state = {
  open: false,
  kind: "convert" as WheelKind,
  mode: "drag" as "drag" | "keyboard",
  paths: [] as string[],
  catalog: null as Catalog | null,
  keys: { convert: "Shift", tools: "Control", actions: "Space" } as KeyHints,
  active: -1,
  /** Picked with Enter mid-drag; the drop that follows is swallowed. */
  done: false,
  busy: false,
};

const $ = (id: string) => document.getElementById(id)!;
const root = $("root");
const segLayer = $("segments");
let clip: Element = $("glass-clip");
let clipCount = 0;
const backdrop = $("backdrop");
const labels = $("labels");
const hub = $("hub");
const hubMain = $("hub-main");
const keybar = $("keybar");

let segs: SVGPathElement[] = [];
let labelEls: HTMLElement[] = [];

function items(): Item[] {
  return state.catalog ? state.catalog[state.kind] : [];
}

const P = (r: number, a: number) => `${(C + r * Math.sin(a)).toFixed(2)},${(C - r * Math.cos(a)).toFixed(2)}`;

/** A ring segment with rounded corners and even gaps between neighbours. */
function petal(i: number, n: number): string {
  const span = (Math.PI * 2) / n;
  const a0 = i * span - span / 2;
  const a1 = i * span + span / 2;
  const off = (r: number) => GAP / 2 / r;
  const r0 = R_IN;
  const r1 = R_OUT;
  const cr = Math.min(12, (span * r0 - GAP) / 3);
  const large = span - 2 * off(r1) > Math.PI ? 1 : 0;
  return [
    `M${P(r0 + cr, a0 + off(r0 + cr))}`,
    `L${P(r1 - cr, a0 + off(r1 - cr))}`,
    `Q${P(r1, a0 + off(r1))} ${P(r1, a0 + off(r1) + cr / r1)}`,
    `A${r1},${r1} 0 ${large} 1 ${P(r1, a1 - off(r1) - cr / r1)}`,
    `Q${P(r1, a1 - off(r1))} ${P(r1 - cr, a1 - off(r1 - cr))}`,
    `L${P(r0 + cr, a1 - off(r0 + cr))}`,
    `Q${P(r0, a1 - off(r0))} ${P(r0, a1 - off(r0) - cr / r0)}`,
    `A${r0},${r0} 0 ${large} 0 ${P(r0, a0 + off(r0) + cr / r0)}`,
    `Q${P(r0, a0 + off(r0))} ${P(r0 + cr, a0 + off(r0 + cr))}`,
    "Z",
  ].join(" ");
}

function ring(r: number) {
  return `M${C - r},${C} a${r},${r} 0 1,0 ${2 * r},0 a${r},${r} 0 1,0 ${-2 * r},0 Z`;
}

function svgEl<K extends keyof SVGElementTagNameMap>(tag: K, attrs: Record<string, string>): SVGElementTagNameMap[K] {
  const node = document.createElementNS(SVG, tag);
  for (const [k, v] of Object.entries(attrs)) node.setAttribute(k, v);
  return node;
}

/** Draw the current wheel, and cut the frosted backdrop to the same shapes. */
function build() {
  segLayer.innerHTML = "";
  labels.innerHTML = "";
  // WebKit doesn't repaint an element when only its clip path's children
  // change, so the old ring would linger. Use a fresh clip path each time.
  const fresh = svgEl("clipPath", { id: `glass-clip-${++clipCount}` });
  clip.replaceWith(fresh);
  clip = fresh;
  segs = [];
  labelEls = [];
  const list = items();
  const n = list.length;
  sizeRing(n);
  // The key bar sits just above the ring.
  keybar.style.top = `${Math.max(4, C - R_OUT - 44)}px`;

  if (n === 0) {
    segLayer.append(svgEl("path", { class: "empty", d: ring((R_OUT + R_IN) / 2) }));
  }

  list.forEach((item, i) => {
    const angle = n === 1 ? 0 : (i * Math.PI * 2) / n;
    const d = n === 1 ? `${ring(R_OUT)} ${ring(R_IN)}` : petal(i, n);
    const seg = svgEl("path", { class: "seg", d, "fill-rule": "evenodd" });
    segLayer.append(seg);
    segs.push(seg);
    clip.append(svgEl("path", { d, "clip-rule": "evenodd" }));

    const label = document.createElement("div");
    label.className = "label";
    const r = n === 1 ? (R_OUT + R_IN) / 2 : R_LABEL;
    label.style.left = `${C + r * Math.sin(angle)}px`;
    label.style.top = `${C - r * Math.cos(angle)}px`;
    if (item.id.startsWith("convert:")) {
      const fmt = document.createElement("div");
      fmt.className = `fmt${item.label.length > 4 ? " long" : ""}`;
      fmt.textContent = item.label;
      label.append(fmt);
    } else {
      label.innerHTML = icon(item.icon, 18);
      const name = document.createElement("div");
      name.className = "name";
      name.textContent = item.label;
      label.append(name);
    }
    labels.append(label);
    labelEls.push(label);
  });

  // The hub and key bar are frosted too.
  clip.append(svgEl("circle", { cx: String(C), cy: String(C), r: String(R_HUB) }));
  const kb = keybar.getBoundingClientRect();
  if (kb.width > 0) {
    clip.append(
      svgEl("rect", {
        x: String(kb.left),
        y: String(kb.top),
        width: String(kb.width),
        height: String(kb.height),
        rx: String(kb.height / 2),
      }),
    );
  }

  backdrop.setAttribute("clip-path", `url(#${clip.id})`);
  // Once this frame is painted, have the window clear what the old ring left.
  requestAnimationFrame(() => api.repaintWheel());

  state.active = -1;
  updateHub();
  updateKeybar();
}

function setActive(i: number) {
  if (i === state.active) return;
  segs[state.active]?.classList.remove("active");
  labelEls[state.active]?.classList.remove("active");
  state.active = i;
  segs[i]?.classList.add("active");
  labelEls[i]?.classList.add("active");
  updateHub();
}

function updateHub() {
  hub.classList.toggle("done", state.done);
  hubMain.classList.remove("error");
  const active = items()[state.active];
  if (state.done) hubMain.textContent = "Let go anywhere";
  else if (active) hubMain.textContent = active.id.startsWith("convert:") ? `To ${active.label}` : active.hint || active.label;
  else if (!state.catalog) hubMain.textContent = state.paths.length ? "" : "Move over the wheel";
  else if (items().length === 0) hubMain.textContent = state.kind === "convert" ? "Nothing to convert" : "No tools here";
  else hubMain.textContent = state.catalog.title;
}

function renderKeybar() {
  keybar.innerHTML = "";
  for (const kind of KINDS) {
    const b = document.createElement("button");
    b.dataset.kind = kind;
    const kbd = document.createElement("kbd");
    kbd.textContent = KEY_SHORT[state.keys[kind]];
    b.append(kbd, KIND_TITLE[kind]);
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      switchKind(kind);
    });
    keybar.append(b);
  }
}

function updateKeybar() {
  keybar.querySelectorAll<HTMLElement>("button").forEach((b) => b.classList.toggle("on", b.dataset.kind === state.kind));
}

function showError(message: string) {
  hubMain.textContent = message;
  hubMain.classList.add("error");
  setTimeout(() => close(), 1600);
}

function switchKind(kind: WheelKind) {
  if (state.done || kind === state.kind || !state.open) return;
  state.kind = kind;
  build();
}

/** Which slice is under a point in window (CSS) pixels: -1 for none or the hub. */
function sliceAt(x: number, y: number): number {
  const n = items().length;
  const dx = x - C;
  const dy = y - C;
  const r = Math.hypot(dx, dy);
  if (n === 0 || r < R_IN - 4 || r > R_OUT + 46) return -1;
  let a = Math.atan2(dx, -dy);
  if (a < 0) a += Math.PI * 2;
  const span = (Math.PI * 2) / n;
  return Math.floor(((a + span / 2) % (Math.PI * 2)) / span);
}

function pointAt(x: number, y: number) {
  if (!state.done) setActive(sliceAt(x, y));
}

async function loadCatalog(paths: string[]) {
  state.paths = paths;
  state.catalog = await api.catalog(paths);
  build();
}

async function apply(item: Item, keepUntilDragEnds: boolean) {
  if (state.busy) return;
  state.busy = true;
  setActive(items().indexOf(item));
  try {
    await api.runItem(item.id, state.paths);
    if (keepUntilDragEnds) {
      state.done = true;
      updateHub();
    } else {
      close();
    }
  } catch (err) {
    showError(String(err));
  } finally {
    state.busy = false;
  }
}

function close() {
  reset();
  api.closeWheel();
}

function reset() {
  state.open = false;
  state.done = false;
  state.busy = false;
  state.catalog = null;
  state.paths = [];
  state.active = -1;
  root.classList.add("closed");
}

function moveActive(delta: number) {
  const n = items().length;
  if (n === 0) return;
  setActive(state.active < 0 ? (delta > 0 ? 0 : n - 1) : (state.active + delta + n) % n);
}

function handleKey(key: string) {
  if (!state.open || state.done) return;
  switch (key) {
    case "right":
    case "down":
      moveActive(1);
      break;
    case "left":
    case "up":
      moveActive(-1);
      break;
    case "enter": {
      const item = items()[state.active];
      if (item) apply(item, state.mode === "drag");
      break;
    }
  }
}

// ---------------------------------------------------------------- events

listen<OpenPayload>("wheel://open", (e) => {
  const p = e.payload;
  const wasOpen = state.open;
  reset();
  state.open = true;
  state.kind = p.kind;
  state.mode = p.mode;
  state.keys = p.keys;
  state.paths = p.paths;
  state.catalog = p.catalog;
  // Reopened while still showing: no new snapshot comes, keep the current one.
  if (p.backdrop) backdrop.setAttribute("href", p.backdrop);
  else if (!wasOpen) backdrop.removeAttribute("href");
  root.classList.toggle("no-backdrop", !backdrop.getAttribute("href"));
  root.classList.remove("closed");
  renderKeybar();
  build();
  if (!p.catalog && p.paths.length) loadCatalog(p.paths);
});

listen<WheelKind>("wheel://kind", (e) => switchKind(e.payload));
listen<string>("wheel://key", (e) => handleKey(e.payload));
listen("wheel://close", () => reset());

getCurrentWebview().onDragDropEvent(async (event) => {
  if (!state.open) return;
  const p = event.payload;
  const ratio = window.devicePixelRatio || 1;
  switch (p.type) {
    case "enter":
      if (!state.catalog && p.paths.length) await loadCatalog(p.paths);
      pointAt(p.position.x / ratio, p.position.y / ratio);
      break;
    case "over":
      pointAt(p.position.x / ratio, p.position.y / ratio);
      break;
    case "drop": {
      if (state.done) {
        close();
        return;
      }
      if (!state.catalog && p.paths.length) await loadCatalog(p.paths);
      const item = items()[sliceAt(p.position.x / ratio, p.position.y / ratio)];
      if (item) await apply(item, false);
      else close();
      break;
    }
    case "leave":
      if (!state.done) setActive(-1);
      break;
  }
});

// Keyboard mode: the wheel has focus.
document.addEventListener("keydown", (e) => {
  if (!state.open || state.mode !== "keyboard") return;
  const byKey = (Object.keys(state.keys) as WheelKind[]).find((k) => {
    const m = state.keys[k];
    const name = m === "Super" ? "Meta" : m === "Space" ? " " : m;
    return name === e.key;
  });
  if (byKey) {
    switchKind(byKey);
    return;
  }
  switch (e.key) {
    case "Escape":
      close();
      break;
    case "Tab":
      e.preventDefault();
      switchKind(KINDS[(KINDS.indexOf(state.kind) + (e.shiftKey ? 2 : 1)) % 3]);
      break;
    case "ArrowRight":
    case "ArrowDown":
      e.preventDefault();
      handleKey("right");
      break;
    case "ArrowLeft":
    case "ArrowUp":
      e.preventDefault();
      handleKey("left");
      break;
    case "Enter":
      e.preventDefault();
      handleKey("enter");
      break;
  }
});

document.addEventListener("mousemove", (e) => {
  if (state.open && state.mode === "keyboard") pointAt(e.clientX, e.clientY);
});

document.addEventListener("click", (e) => {
  if (!state.open || state.mode !== "keyboard") return;
  const item = items()[sliceAt(e.clientX, e.clientY)];
  if (item) apply(item, false);
  else if (Math.hypot(e.clientX - C, e.clientY - C) > R_OUT + 10) close();
});

window.addEventListener("blur", () => {
  if (state.open && state.mode === "keyboard" && !state.busy) close();
});
