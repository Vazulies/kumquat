import { el, EditorJob } from "../shared/api";
import { Frame } from "./frame";

function choices<T>(options: { value: T; label: string }[], initial: T, onPick: (v: T) => void) {
  const wrap = el("div", { class: "choices" });
  for (const o of options) {
    const b = el("button", { textContent: o.label, className: o.value === initial ? "on" : "" });
    b.addEventListener("click", () => {
      wrap.querySelectorAll("button").forEach((x) => x.classList.remove("on"));
      b.classList.add("on");
      onPick(o.value);
    });
    wrap.append(b);
  }
  return wrap;
}

/** Small forms for tools that need one or two numbers. */
export function mountParams(job: EditorJob, frame: Frame) {
  const form = el("div", { class: "params" });
  frame.body.append(form);
  const tool = job.item.id.replace("tool:", "");

  if (tool === "image.resize") {
    let mode: "percent" | "width" = "percent";
    let percent = 50;
    const pct = el("input", { type: "range", min: "5", max: "200", step: "5", value: "50" });
    const pctLabel = el("span", { textContent: "50%" });
    const width = el("input", { type: "number", min: "1", placeholder: "e.g. 1920" });
    const pctRow = el("label", { class: "row" }, el("span", {}, "Scale ", pctLabel), pct);
    const widthRow = el("label", { class: "row" }, "Width in pixels (height follows)", width);
    widthRow.hidden = true;
    pct.addEventListener("input", () => {
      percent = Number(pct.value);
      pctLabel.textContent = `${percent}%`;
    });
    form.append(
      choices(
        [
          { value: "percent", label: "By percentage" },
          { value: "width", label: "To a width" },
        ],
        "percent",
        (v) => {
          mode = v as typeof mode;
          pctRow.hidden = mode !== "percent";
          widthRow.hidden = mode !== "width";
        },
      ),
      pctRow,
      widthRow,
    );
    frame.apply.textContent = "Resize";
    frame.apply.addEventListener("click", () => {
      if (mode === "width") {
        const w = Number(width.value);
        if (!w || w < 1) return frame.status("Enter a width", true);
        frame.submit({ width: w });
      } else frame.submit({ percent });
    });
  } else if (tool === "image.background") {
    let color = "#ffffff";
    const picker = el("input", { type: "color", value: color });
    picker.style.cssText = "width:64px;height:36px;border:0;background:none;padding:0";
    picker.addEventListener("input", () => (color = picker.value));
    form.append(
      el("label", { class: "row" }, "Fill transparent areas with", picker),
      choices(
        [
          { value: "#ffffff", label: "White" },
          { value: "#000000", label: "Black" },
          { value: "#f2f2f2", label: "Light grey" },
          { value: "#ff8a1f", label: "Orange" },
        ],
        "#ffffff",
        (v) => {
          color = v;
          picker.value = v;
        },
      ),
    );
    frame.apply.textContent = "Fill";
    frame.apply.addEventListener("click", () => frame.submit({ color }));
  } else if (tool === "video.speed" || tool === "audio.speed") {
    let factor = 2;
    const custom = el("input", { type: "number", min: "0.1", max: "10", step: "0.05", value: "2" });
    custom.addEventListener("input", () => (factor = Number(custom.value)));
    form.append(
      el("label", { class: "row" }, "Playback speed"),
      choices(
        [0.25, 0.5, 0.75, 1.25, 1.5, 2, 3, 4].map((v) => ({ value: v, label: `${v}×` })),
        2,
        (v) => {
          factor = v;
          custom.value = String(v);
        },
      ),
      el("label", { class: "row" }, "Or exactly", custom),
      el("div", { class: "note", textContent: "Sound keeps its pitch." }),
    );
    frame.apply.textContent = "Change speed";
    frame.apply.addEventListener("click", () => {
      if (!(factor >= 0.1 && factor <= 10)) return frame.status("Pick a speed from 0.1× to 10×", true);
      frame.submit({ factor });
    });
  } else if (tool === "subtitle.shift") {
    const secs = el("input", { type: "number", step: "0.1", value: "0" });
    form.append(
      el("label", { class: "row" }, "Move every subtitle by (seconds)", secs),
      choices(
        [-2, -1, -0.5, 0.5, 1, 2].map((v) => ({ value: v, label: `${v > 0 ? "+" : ""}${v}s` })),
        0,
        (v) => (secs.value = String(Number(secs.value) + v)),
      ),
      el("div", { class: "note", textContent: "Negative numbers show subtitles earlier." }),
    );
    frame.apply.textContent = "Shift";
    frame.apply.addEventListener("click", () => {
      const seconds = Number(secs.value);
      if (!seconds) return frame.status("Enter how far to move them", true);
      frame.submit({ seconds });
    });
  } else {
    frame.status(`Nothing to set for ${job.item.label}`, true);
  }
}
