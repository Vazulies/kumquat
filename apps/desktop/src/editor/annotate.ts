import { api, el, EditorJob } from "../shared/api";
import { icon } from "../shared/icons";
import { fitImage } from "./cropper";
import { Frame, loading, segmented } from "./frame";

type Tool = "pen" | "highlighter" | "arrow" | "rect" | "text";

interface Shape {
  tool: Tool;
  color: string;
  /** Stroke width as a fraction of the image width, so it scales on export. */
  size: number;
  /** Points normalised to 0–1. */
  points: [number, number][];
  text?: string;
}

const COLORS = ["#ff3b30", "#ff9500", "#ffcc00", "#34c759", "#0a84ff", "#af52de", "#111111", "#ffffff"];
const SIZES = { S: 0.003, M: 0.006, L: 0.012 };

function draw(ctx: CanvasRenderingContext2D, shapes: Shape[], W: number, H: number) {
  ctx.clearRect(0, 0, W, H);
  for (const s of shapes) {
    const pts = s.points.map(([x, y]) => [x * W, y * H] as [number, number]);
    const lw = Math.max(1, s.size * W);
    ctx.save();
    ctx.strokeStyle = s.color;
    ctx.fillStyle = s.color;
    ctx.lineCap = "round";
    ctx.lineJoin = "round";
    ctx.lineWidth = lw;
    switch (s.tool) {
      case "highlighter":
        ctx.globalAlpha = 0.35;
        ctx.lineWidth = lw * 4;
        ctx.lineCap = "butt";
      // falls through
      case "pen":
        ctx.beginPath();
        pts.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
        if (pts.length === 1) ctx.lineTo(pts[0][0] + 0.1, pts[0][1]);
        ctx.stroke();
        break;
      case "rect": {
        const [[x0, y0], [x1, y1]] = [pts[0], pts[pts.length - 1]];
        ctx.strokeRect(Math.min(x0, x1), Math.min(y0, y1), Math.abs(x1 - x0), Math.abs(y1 - y0));
        break;
      }
      case "arrow": {
        const [[x0, y0], [x1, y1]] = [pts[0], pts[pts.length - 1]];
        const angle = Math.atan2(y1 - y0, x1 - x0);
        const head = lw * 4.5;
        ctx.beginPath();
        ctx.moveTo(x0, y0);
        ctx.lineTo(x1 - Math.cos(angle) * head * 0.6, y1 - Math.sin(angle) * head * 0.6);
        ctx.stroke();
        ctx.beginPath();
        ctx.moveTo(x1, y1);
        ctx.lineTo(x1 - head * Math.cos(angle - 0.45), y1 - head * Math.sin(angle - 0.45));
        ctx.lineTo(x1 - head * Math.cos(angle + 0.45), y1 - head * Math.sin(angle + 0.45));
        ctx.closePath();
        ctx.fill();
        break;
      }
      case "text": {
        const px = Math.max(10, lw * 7);
        ctx.font = `600 ${px}px Inter, Cantarell, Ubuntu, sans-serif`;
        ctx.textBaseline = "top";
        ctx.lineWidth = Math.max(2, px / 8);
        ctx.strokeStyle = s.color === "#ffffff" ? "rgba(0,0,0,0.6)" : "rgba(255,255,255,0.85)";
        ctx.strokeText(s.text ?? "", pts[0][0], pts[0][1]);
        ctx.fillText(s.text ?? "", pts[0][0], pts[0][1]);
        break;
      }
    }
    ctx.restore();
  }
}

export async function mountAnnotate(job: EditorJob, frame: Frame) {
  const done = loading(frame.body);
  const preview = await api.imagePreview(job.paths[0], 1600);
  done();
  const natW = preview.width;
  const natH = preview.height;

  const stage = el("div", { class: "stage" });
  const inner = el("div", { class: "stage-inner" });
  const img = el("img", { src: preview.src, draggable: false });
  const canvas = el("canvas", { class: "draw-canvas" });
  inner.append(img, canvas);
  stage.append(inner);
  frame.body.append(stage);
  fitImage(img, stage, natW, natH);

  const shapes: Shape[] = [];
  let tool: Tool = "pen";
  let color = COLORS[0];
  let size = SIZES.M;

  const ctx = canvas.getContext("2d")!;
  const redraw = () => {
    const ratio = window.devicePixelRatio || 1;
    const w = img.clientWidth;
    const h = img.clientHeight;
    if (canvas.width !== Math.round(w * ratio)) {
      canvas.width = Math.round(w * ratio);
      canvas.height = Math.round(h * ratio);
      canvas.style.width = `${w}px`;
      canvas.style.height = `${h}px`;
    }
    draw(ctx, shapes, canvas.width, canvas.height);
    frame.status(shapes.length ? `${shapes.length} mark${shapes.length > 1 ? "s" : ""}` : "Draw on the image");
  };
  new ResizeObserver(redraw).observe(img);

  const norm = (e: PointerEvent): [number, number] => {
    const r = canvas.getBoundingClientRect();
    return [(e.clientX - r.left) / r.width, (e.clientY - r.top) / r.height];
  };

  canvas.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    const p = norm(e);
    if (tool === "text") {
      placeText(p, e);
      return;
    }
    const shape: Shape = { tool, color, size, points: [p] };
    shapes.push(shape);
    canvas.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      const q = norm(ev);
      if (tool === "pen" || tool === "highlighter") shape.points.push(q);
      else shape.points = [shape.points[0], q];
      redraw();
    };
    const up = () => {
      canvas.removeEventListener("pointermove", move);
      canvas.removeEventListener("pointerup", up);
      if (shape.points.length < 2 && (tool === "rect" || tool === "arrow")) shapes.pop();
      redraw();
    };
    canvas.addEventListener("pointermove", move);
    canvas.addEventListener("pointerup", up);
  });

  function placeText(p: [number, number], e: PointerEvent) {
    const r = inner.getBoundingClientRect();
    const input = el("input", { type: "text", class: "text-input", placeholder: "Type, then Enter" });
    input.style.left = `${e.clientX - r.left}px`;
    input.style.top = `${e.clientY - r.top}px`;
    inner.append(input);
    setTimeout(() => input.focus(), 0);
    const commit = () => {
      if (input.value.trim()) {
        shapes.push({ tool: "text", color, size, points: [p], text: input.value });
        redraw();
      }
      input.remove();
    };
    input.addEventListener("keydown", (ev) => {
      if (ev.key === "Enter") commit();
      if (ev.key === "Escape") input.remove();
      ev.stopPropagation();
    });
    input.addEventListener("blur", commit, { once: true });
  }

  // Toolbar
  frame.toolbar.hidden = false;
  frame.toolbar.append(
    segmented<Tool>(
      [
        { value: "pen", label: "Pen", html: icon("pen", 16) },
        { value: "highlighter", label: "Highlighter", html: icon("highlighter", 16) },
        { value: "arrow", label: "Arrow", html: icon("arrow", 16) },
        { value: "rect", label: "Box", html: icon("rect", 16) },
        { value: "text", label: "Text", html: icon("text", 16) },
      ],
      "pen",
      (v) => (tool = v),
    ),
    el("div", { class: "sep" }),
  );
  for (const c of COLORS) {
    const b = el("button", { class: `swatch${c === color ? " on" : ""}`, title: c });
    b.style.background = c;
    b.addEventListener("click", () => {
      color = c;
      frame.toolbar.querySelectorAll(".swatch").forEach((s) => s.classList.remove("on"));
      b.classList.add("on");
    });
    frame.toolbar.append(b);
  }
  const undo = el("button", { class: "btn icon-only", title: "Undo" });
  undo.innerHTML = icon("undo", 16);
  undo.addEventListener("click", () => {
    shapes.pop();
    redraw();
  });
  const clear = el("button", { class: "btn icon-only", title: "Clear all" });
  clear.innerHTML = icon("trash", 16);
  clear.addEventListener("click", () => {
    shapes.length = 0;
    redraw();
  });
  frame.toolbar.append(
    el("div", { class: "sep" }),
    segmented(
      (Object.keys(SIZES) as (keyof typeof SIZES)[]).map((k) => ({ value: k, label: k, title: `Size ${k}` })),
      "M",
      (v) => (size = SIZES[v]),
    ),
    el("div", { class: "sep" }),
    undo,
    clear,
  );
  document.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === "z") {
      shapes.pop();
      redraw();
    }
  });

  frame.apply.textContent = "Save copy";
  frame.apply.addEventListener("click", () => {
    if (!shapes.length) {
      frame.status("Draw something first", true);
      return;
    }
    // Render the marks at the original's full size.
    const out = document.createElement("canvas");
    out.width = natW;
    out.height = natH;
    draw(out.getContext("2d")!, shapes, natW, natH);
    frame.submit({ overlay: out.toDataURL("image/png") });
  });
}
