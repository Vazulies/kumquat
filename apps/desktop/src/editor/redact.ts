import { api, el, EditorJob } from "../shared/api";
import { icon } from "../shared/icons";
import { fitImage } from "./cropper";
import { Frame, loading } from "./frame";

interface Box {
  page: number;
  x: number;
  y: number;
  w: number;
  h: number;
}

export async function mountRedact(job: EditorJob, frame: Frame) {
  const path = job.paths[0];
  const done = loading(frame.body);
  const count = await api.pdfInfo(path);
  done();

  const boxes: Box[] = [];
  let page = 0;

  const stage = el("div", { class: "stage" });
  const inner = el("div", { class: "stage-inner" });
  const img = el("img", { draggable: false });
  const layer = el("div", { class: "redact-layer" });
  inner.append(img, layer);
  stage.append(inner);
  frame.body.append(stage);

  const prev = el("button", { class: "btn icon-only", title: "Previous page" });
  prev.innerHTML = icon("chevronLeft", 16);
  const next = el("button", { class: "btn icon-only", title: "Next page" });
  next.innerHTML = icon("chevronRight", 16);
  const where = el("span");
  where.style.cssText = "font-size:13px;min-width:90px;text-align:center";
  const hint = el("span", { textContent: "Drag over anything you want blacked out." });
  hint.style.cssText = "font-size:12.5px;color:var(--text-2);margin-left:8px";
  frame.toolbar.hidden = false;
  frame.toolbar.append(prev, where, next, hint);

  const summary = () => {
    const pages = new Set(boxes.map((b) => b.page)).size;
    frame.status(
      boxes.length
        ? `${boxes.length} area${boxes.length > 1 ? "s" : ""} on ${pages} page${pages > 1 ? "s" : ""}. Marked pages become images, so hidden text is really gone.`
        : "Nothing marked yet",
    );
  };

  const drawBoxes = () => {
    layer.innerHTML = "";
    for (const b of boxes.filter((b) => b.page === page)) {
      const node = el("div", { class: "redact-box" });
      Object.assign(node.style, {
        left: `${b.x * 100}%`,
        top: `${b.y * 100}%`,
        width: `${b.w * 100}%`,
        height: `${b.h * 100}%`,
      });
      const del = el("button", { title: "Remove" });
      del.innerHTML = icon("close", 12);
      del.addEventListener("pointerdown", (e) => e.stopPropagation());
      del.addEventListener("click", () => {
        boxes.splice(boxes.indexOf(b), 1);
        drawBoxes();
      });
      node.append(del);
      layer.append(node);
    }
    summary();
  };

  const show = async (index: number) => {
    page = index;
    where.textContent = `Page ${page + 1} of ${count}`;
    prev.disabled = page === 0;
    next.disabled = page === count - 1;
    const src = await api.pdfPage(path, page, 1100);
    await new Promise<void>((resolve) => {
      img.onload = () => resolve();
      img.src = src;
    });
    // Pages can differ in size, so refit for each one.
    fitImage(img, stage, img.naturalWidth, img.naturalHeight);
    drawBoxes();
  };
  prev.addEventListener("click", () => page > 0 && show(page - 1));
  next.addEventListener("click", () => page < count - 1 && show(page + 1));

  layer.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    const r = layer.getBoundingClientRect();
    const nx = (cx: number) => Math.min(1, Math.max(0, (cx - r.left) / r.width));
    const ny = (cy: number) => Math.min(1, Math.max(0, (cy - r.top) / r.height));
    const x0 = nx(e.clientX);
    const y0 = ny(e.clientY);
    const box: Box = { page, x: x0, y: y0, w: 0, h: 0 };
    boxes.push(box);
    layer.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => {
      const x1 = nx(ev.clientX);
      const y1 = ny(ev.clientY);
      box.x = Math.min(x0, x1);
      box.y = Math.min(y0, y1);
      box.w = Math.abs(x1 - x0);
      box.h = Math.abs(y1 - y0);
      drawBoxes();
    };
    const up = () => {
      layer.removeEventListener("pointermove", move);
      layer.removeEventListener("pointerup", up);
      if (box.w < 0.004 || box.h < 0.004) boxes.splice(boxes.indexOf(box), 1);
      drawBoxes();
    };
    layer.addEventListener("pointermove", move);
    layer.addEventListener("pointerup", up);
  });

  await show(0);

  frame.apply.textContent = "Redact";
  frame.apply.addEventListener("click", () => {
    if (!boxes.length) {
      frame.status("Mark at least one area", true);
      return;
    }
    frame.submit({ boxes });
  });
}
