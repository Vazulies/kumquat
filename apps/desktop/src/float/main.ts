import "../shared/base.css";
import "./float.css";
import { startDrag } from "@crabnebula/tauri-plugin-drag";
import { api, extOf, fileName } from "../shared/api";
import { icon } from "../shared/icons";

async function main() {
  const ctx = await api.floatContext();
  if (!ctx) {
    await api.floatClose();
    return;
  }
  const thumb = document.getElementById("thumb")!;
  const name = document.getElementById("name")!;
  const count = document.getElementById("count")!;
  const close = document.getElementById("close")!;
  const card = document.getElementById("card")!;

  const leave = () => api.floatClose();
  close.innerHTML = icon("close", 14);
  close.addEventListener("click", leave);

  if (ctx.preview) {
    thumb.style.backgroundImage = `url(${ctx.preview})`;
    thumb.classList.add("photo");
  } else {
    const ext = extOf(ctx.paths[0]).toUpperCase() || "FILE";
    thumb.innerHTML = `${icon("format", 46)}<span></span>`;
    thumb.querySelector("span")!.textContent = ext.slice(0, 5);
  }
  name.textContent = fileName(ctx.paths[0]);
  count.textContent = ctx.paths.length > 1 ? `${ctx.paths.length} items` : "Float";

  card.addEventListener("pointerdown", async (e) => {
    if (e.button !== 0) return;
    e.preventDefault();
    card.classList.add("dragging");
    try {
      await startDrag({ item: ctx.paths, icon: ctx.icon, mode: "copy" }, (result) => {
        card.classList.remove("dragging");
        if (result.result === "Dropped") leave();
      });
    } catch (err) {
      card.classList.remove("dragging");
      console.error(err);
    }
  });

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") leave();
  });
}

main();
