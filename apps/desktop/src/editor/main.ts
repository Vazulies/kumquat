import "./editor.css";
import { api, el } from "../shared/api";
import { mountAnnotate } from "./annotate";
import { mountImageCrop, mountVideoCrop } from "./crop";
import { createFrame } from "./frame";
import { mountPages } from "./pages";
import { mountParams } from "./params";
import { mountRedact } from "./redact";
import { mountTrim } from "./trim";

const root = document.getElementById("app")!;

async function main() {
  const job = await api.editorContext();
  if (!job) {
    root.append(el("div", { class: "loading", textContent: "This editor has already finished." }));
    return;
  }
  const frame = createFrame(root, job);
  try {
    switch (job.item.editor) {
      case "params":
        mountParams(job, frame);
        break;
      case "image-crop":
        await mountImageCrop(job, frame);
        break;
      case "image-annotate":
        await mountAnnotate(job, frame);
        break;
      case "video-crop":
        await mountVideoCrop(job, frame);
        break;
      case "media-trim":
        await mountTrim(job, frame);
        break;
      case "pdf-pages":
        await mountPages(job, frame);
        break;
      case "pdf-redact":
        await mountRedact(job, frame);
        break;
      default:
        frame.status(`Unknown editor ${job.item.editor}`, true);
    }
  } catch (err) {
    frame.body.querySelector(".loading")?.remove();
    frame.status(String(err), true);
    frame.apply.disabled = true;
  }
}

main();
