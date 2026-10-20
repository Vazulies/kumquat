import { api, el, EditorJob, formatTime } from "../shared/api";
import { Frame, loading } from "./frame";

function parseTime(text: string): number | null {
  const parts = text.trim().split(":").map(Number);
  if (parts.some((p) => Number.isNaN(p))) return null;
  return parts.reduce((acc, p) => acc * 60 + p, 0);
}

export async function mountTrim(job: EditorJob, frame: Frame) {
  const path = job.paths[0];
  const done = loading(frame.body);
  const info = await api.mediaInfo(path);
  const duration = info.duration ?? 0;
  if (duration <= 0) {
    done();
    frame.status("Couldn't read how long this file is.", true);
    frame.apply.disabled = true;
    return;
  }
  const isVideo = !!info.videoCodec;
  let start = 0;
  let end = duration;

  const wrap = el("div", { class: "trim" });
  const timeline = el("div", { class: "timeline" });
  const shadeL = el("div", { class: "shade" });
  const shadeR = el("div", { class: "shade" });
  const range = el("div", { class: "range" });
  const gripL = el("div", { class: "grip" });
  const gripR = el("div", { class: "grip" });
  range.append(gripL, gripR);
  gripL.style.left = "-3px";
  gripR.style.right = "-3px";

  let firstFrame: HTMLElement | null = null;
  let lastFrame: HTMLElement | null = null;
  let firstCap: HTMLElement | null = null;
  let lastCap: HTMLElement | null = null;

  if (isVideo) {
    const preview = el("div", { class: "trim-preview" });
    const fig = (label: string) => {
      const pic = el("div");
      const cap = el("figcaption", {}, el("span", { textContent: label }), el("span"));
      preview.append(el("figure", {}, pic, cap));
      return [pic, cap.lastElementChild as HTMLElement] as const;
    };
    [firstFrame, firstCap] = fig("Starts at");
    [lastFrame, lastCap] = fig("Ends at");
    wrap.append(preview);
    const strip = el("div", { class: "strip" });
    timeline.append(strip);
    const count = 10;
    for (let i = 0; i < count; i++) strip.append(el("div"));
    // Fill the filmstrip one frame at a time so the editor opens quickly.
    (async () => {
      for (let i = 0; i < count; i++) {
        const t = (duration * (i + 0.5)) / count;
        const src = await api.mediaFrame(path, t, 160).catch(() => "");
        (strip.children[i] as HTMLElement).style.backgroundImage = src ? `url(${src})` : "";
      }
    })();
  } else {
    const canvas = el("canvas");
    timeline.append(canvas);
    timeline.style.height = "140px";
    api.mediaWaveform(path, 600).then((peaks) => {
      const ratio = window.devicePixelRatio || 1;
      canvas.width = timeline.clientWidth * ratio;
      canvas.height = timeline.clientHeight * ratio;
      const ctx = canvas.getContext("2d")!;
      const mid = canvas.height / 2;
      const bar = canvas.width / peaks.length;
      ctx.fillStyle = "rgba(255, 255, 255, 0.75)";
      peaks.forEach((p, i) => {
        const h = Math.max(1, p * (mid - 4));
        ctx.fillRect(i * bar, mid - h, Math.max(1, bar - 1), h * 2);
      });
    });
    const spacer = el("div");
    spacer.style.flex = "1";
    wrap.append(spacer);
  }

  timeline.append(shadeL, shadeR, range);
  wrap.append(timeline);

  const startInput = el("input", { type: "text" });
  const endInput = el("input", { type: "text" });
  const len = el("span", { class: "len" });
  wrap.append(
    el("div", { class: "times" }, el("span", { textContent: "Start" }), startInput, el("span", { textContent: "End" }), endInput, len),
  );
  done();
  frame.body.append(wrap);

  let frameTimer = 0;
  const refreshFrames = (which: "start" | "end" | "both") => {
    if (!isVideo) return;
    clearTimeout(frameTimer);
    frameTimer = window.setTimeout(async () => {
      if (which !== "end" && firstFrame) {
        firstFrame.style.backgroundImage = `url(${await api.mediaFrame(path, start, 480)})`;
      }
      if (which !== "start" && lastFrame) {
        lastFrame.style.backgroundImage = `url(${await api.mediaFrame(path, Math.max(0, end - 0.05), 480)})`;
      }
    }, 120);
  };

  const render = () => {
    const w = timeline.clientWidth;
    const l = (start / duration) * w;
    const r = (end / duration) * w;
    shadeL.style.cssText = `left:0;width:${l}px`;
    shadeR.style.cssText = `left:${r}px;right:0`;
    range.style.left = `${l}px`;
    range.style.width = `${Math.max(6, r - l)}px`;
    if (document.activeElement !== startInput) startInput.value = formatTime(start);
    if (document.activeElement !== endInput) endInput.value = formatTime(end);
    if (firstCap) firstCap.textContent = formatTime(start);
    if (lastCap) lastCap.textContent = formatTime(end);
    len.textContent = `Keeps ${formatTime(end - start)} of ${formatTime(duration)}`;
    frame.status("");
  };
  new ResizeObserver(render).observe(timeline);

  const timeAt = (clientX: number) => {
    const r = timeline.getBoundingClientRect();
    return Math.min(duration, Math.max(0, ((clientX - r.left) / r.width) * duration));
  };
  const minLen = Math.min(0.1, duration / 2);

  const drag = (e: PointerEvent, mode: "start" | "end" | "move") => {
    e.preventDefault();
    e.stopPropagation();
    const target = e.currentTarget as HTMLElement;
    target.setPointerCapture(e.pointerId);
    const t0 = timeAt(e.clientX);
    const s0 = start;
    const e0 = end;
    const move = (ev: PointerEvent) => {
      const t = timeAt(ev.clientX);
      if (mode === "start") start = Math.min(t, end - minLen);
      else if (mode === "end") end = Math.max(t, start + minLen);
      else {
        const span = e0 - s0;
        start = Math.min(Math.max(0, s0 + t - t0), duration - span);
        end = start + span;
      }
      render();
      refreshFrames(mode === "move" ? "both" : mode);
    };
    const up = () => {
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", up);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", up);
  };
  gripL.addEventListener("pointerdown", (e) => drag(e, "start"));
  gripR.addEventListener("pointerdown", (e) => drag(e, "end"));
  range.addEventListener("pointerdown", (e) => drag(e, "move"));

  const fromInput = (input: HTMLInputElement, which: "start" | "end") => {
    const t = parseTime(input.value);
    if (t == null) return;
    if (which === "start") start = Math.min(Math.max(0, t), end - minLen);
    else end = Math.max(Math.min(duration, t), start + minLen);
    render();
    refreshFrames(which);
  };
  startInput.addEventListener("change", () => fromInput(startInput, "start"));
  endInput.addEventListener("change", () => fromInput(endInput, "end"));

  render();
  refreshFrames("both");

  frame.apply.textContent = "Trim";
  frame.apply.addEventListener("click", () => frame.submit({ start, end }));
}
