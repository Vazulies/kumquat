import { el } from "../shared/api";

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

type Drag = "move" | "new" | "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";
const HANDLES: Drag[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

const fitters = new WeakMap<HTMLImageElement, ResizeObserver>();

/** Size an image to fit the stage while keeping its aspect ratio. */
export function fitImage(img: HTMLImageElement, stage: HTMLElement, natW: number, natH: number) {
  fitters.get(img)?.disconnect();
  const fit = () => {
    const pad = 36;
    const maxW = Math.max(50, stage.clientWidth - pad);
    const maxH = Math.max(50, stage.clientHeight - pad);
    const scale = Math.min(maxW / natW, maxH / natH, 4);
    img.style.width = `${Math.round(natW * scale)}px`;
    img.style.height = `${Math.round(natH * scale)}px`;
  };
  fit();
  const observer = new ResizeObserver(fit);
  observer.observe(stage);
  fitters.set(img, observer);
}

/** A draggable crop rectangle over an image, in the image's natural pixels. */
export class Cropper {
  rect: Rect;
  aspect: number | null = null;
  private box: HTMLElement;
  private clip: HTMLElement;

  constructor(
    host: HTMLElement,
    private img: HTMLImageElement,
    private natW: number,
    private natH: number,
    private onChange: (r: Rect) => void,
  ) {
    this.rect = { x: natW * 0.1, y: natH * 0.1, w: natW * 0.8, h: natH * 0.8 };
    this.clip = el("div", { class: "crop-clip" });
    this.box = el("div", { class: "crop-box" });
    for (const h of HANDLES) {
      const node = el("div", { class: "handle" });
      node.dataset.drag = h;
      node.style.cursor = `${h}-resize`;
      const pos: Record<string, [number, number]> = {
        nw: [0, 0], n: [50, 0], ne: [100, 0], e: [100, 50],
        se: [100, 100], s: [50, 100], sw: [0, 100], w: [0, 50],
      };
      node.style.left = `${pos[h][0]}%`;
      node.style.top = `${pos[h][1]}%`;
      this.box.append(node);
    }
    this.box.dataset.drag = "move";
    this.clip.dataset.drag = "new";
    this.clip.append(this.box);
    host.append(this.clip);
    this.clip.addEventListener("pointerdown", (e) => this.start(e));
    new ResizeObserver(() => this.render()).observe(img);
    this.render();
  }

  private scale() {
    return this.img.clientWidth / this.natW || 1;
  }

  private toNatural(e: PointerEvent): [number, number] {
    const r = this.img.getBoundingClientRect();
    const s = this.scale();
    return [
      Math.min(this.natW, Math.max(0, (e.clientX - r.left) / s)),
      Math.min(this.natH, Math.max(0, (e.clientY - r.top) / s)),
    ];
  }

  private start(e: PointerEvent) {
    const target = (e.target as HTMLElement).closest<HTMLElement>("[data-drag]");
    if (!target) return;
    e.preventDefault();
    const mode = target.dataset.drag as Drag;
    const [px, py] = this.toNatural(e);
    const start = { ...this.rect };
    this.clip.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      const [x, y] = this.toNatural(ev);
      this.update(mode, start, px, py, x, y);
    };
    const up = () => {
      this.clip.removeEventListener("pointermove", move);
      this.clip.removeEventListener("pointerup", up);
    };
    this.clip.addEventListener("pointermove", move);
    this.clip.addEventListener("pointerup", up);
  }

  private update(mode: Drag, s: Rect, px: number, py: number, x: number, y: number) {
    const min = Math.max(8, Math.min(this.natW, this.natH) * 0.01);
    let { x: rx, y: ry, w, h } = s;
    if (mode === "move") {
      rx = Math.min(this.natW - w, Math.max(0, s.x + x - px));
      ry = Math.min(this.natH - h, Math.max(0, s.y + y - py));
    } else if (mode === "new") {
      rx = Math.min(px, x);
      ry = Math.min(py, y);
      w = Math.abs(x - px);
      h = Math.abs(y - py);
      if (this.aspect) {
        h = w / this.aspect;
        if (y < py) ry = py - h;
      }
    } else {
      let left = s.x, top = s.y, right = s.x + s.w, bottom = s.y + s.h;
      if (mode.includes("w")) left = Math.min(x, right - min);
      if (mode.includes("e")) right = Math.max(x, left + min);
      if (mode.includes("n")) top = Math.min(y, bottom - min);
      if (mode.includes("s")) bottom = Math.max(y, top + min);
      rx = left;
      ry = top;
      w = right - left;
      h = bottom - top;
      if (this.aspect) {
        if (mode === "n" || mode === "s") {
          w = h * this.aspect;
        } else {
          h = w / this.aspect;
          if (mode.includes("n")) ry = bottom - h;
        }
      }
    }
    // Keep inside the image.
    w = Math.max(min, Math.min(w, this.natW));
    h = Math.max(min, Math.min(h, this.natH));
    rx = Math.min(Math.max(0, rx), this.natW - w);
    ry = Math.min(Math.max(0, ry), this.natH - h);
    this.rect = { x: rx, y: ry, w, h };
    this.render();
  }

  setAspect(aspect: number | null) {
    this.aspect = aspect;
    if (aspect) {
      let w = this.natW;
      let h = w / aspect;
      if (h > this.natH) {
        h = this.natH;
        w = h * aspect;
      }
      this.rect = { x: (this.natW - w) / 2, y: (this.natH - h) / 2, w, h };
    }
    this.render();
  }

  render() {
    const s = this.scale();
    Object.assign(this.box.style, {
      left: `${this.rect.x * s}px`,
      top: `${this.rect.y * s}px`,
      width: `${this.rect.w * s}px`,
      height: `${this.rect.h * s}px`,
    });
    this.onChange(this.rounded());
  }

  rounded(): Rect {
    return {
      x: Math.round(this.rect.x),
      y: Math.round(this.rect.y),
      w: Math.round(this.rect.w),
      h: Math.round(this.rect.h),
    };
  }
}

export const ASPECTS: { value: string; label: string; ratio: (w: number, h: number) => number | null }[] = [
  { value: "free", label: "Free", ratio: () => null },
  { value: "orig", label: "Original", ratio: (w, h) => w / h },
  { value: "1:1", label: "1:1", ratio: () => 1 },
  { value: "4:3", label: "4:3", ratio: () => 4 / 3 },
  { value: "3:2", label: "3:2", ratio: () => 3 / 2 },
  { value: "16:9", label: "16:9", ratio: () => 16 / 9 },
  { value: "9:16", label: "9:16", ratio: () => 9 / 16 },
];
