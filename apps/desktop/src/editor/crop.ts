import { api, el, EditorJob, formatTime } from "../shared/api";
import { ASPECTS, Cropper, fitImage } from "./cropper";
import { Frame, loading, segmented } from "./frame";

function mountCropper(frame: Frame, src: string, natW: number, natH: number): Cropper {
  const stage = el("div", { class: "stage" });
  const inner = el("div", { class: "stage-inner" });
  const img = el("img", { src, draggable: false });
  inner.append(img);
  stage.append(inner);
  frame.body.append(stage);
  fitImage(img, stage, natW, natH);
  const cropper = new Cropper(inner, img, natW, natH, (r) => frame.status(`${r.w} × ${r.h} px`));

  frame.toolbar.hidden = false;
  frame.toolbar.append(
    segmented(
      ASPECTS.map((a) => ({ value: a.value, label: a.label })),
      "free",
      (v) => cropper.setAspect(ASPECTS.find((a) => a.value === v)!.ratio(natW, natH)),
    ),
  );
  return cropper;
}

export async function mountImageCrop(job: EditorJob, frame: Frame) {
  const done = loading(frame.body);
  const preview = await api.imagePreview(job.paths[0], 1600);
  done();
  const cropper = mountCropper(frame, preview.src, preview.width, preview.height);
  frame.apply.textContent = "Crop";
  frame.apply.addEventListener("click", () => {
    const r = cropper.rounded();
    frame.submit({ x: r.x, y: r.y, width: r.w, height: r.h });
  });
}

export async function mountVideoCrop(job: EditorJob, frame: Frame) {
  const path = job.paths[0];
  const done = loading(frame.body);
  const info = await api.mediaInfo(path);
  if (!info.width || !info.height) {
    done();
    frame.status("This file has no picture to crop.", true);
    frame.apply.disabled = true;
    return;
  }
  const duration = info.duration ?? 0;
  const at = Math.min(1, duration / 2);
  const src = await api.mediaFrame(path, at, 1280);
  done();
  const cropper = mountCropper(frame, src, info.width, info.height);
  const img = frame.body.querySelector("img")!;

  // Scrub to pick a representative frame.
  const scrub = el("input", { type: "range", min: "0", max: String(duration), step: "0.1", value: String(at) });
  scrub.style.width = "220px";
  scrub.style.accentColor = "#ffffff";
  const when = el("span", { textContent: formatTime(at) });
  when.style.cssText = "font-size:12px;color:var(--text-2);font-family:var(--mono)";
  let timer = 0;
  scrub.addEventListener("input", () => {
    when.textContent = formatTime(Number(scrub.value));
    clearTimeout(timer);
    timer = window.setTimeout(async () => {
      img.src = await api.mediaFrame(path, Number(scrub.value), 1280);
    }, 120);
  });
  frame.toolbar.append(el("div", { class: "sep" }), scrub, when);

  frame.apply.textContent = "Crop";
  frame.apply.addEventListener("click", () => {
    const r = cropper.rounded();
    frame.submit({ x: r.x, y: r.y, width: r.w, height: r.h });
  });
}
