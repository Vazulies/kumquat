import { api, el, EditorJob } from "../shared/api";
import { icon } from "../shared/icons";
import { Frame, loading } from "./frame";

interface Card {
  index: number;
  rotate: number;
  removed: boolean;
  node: HTMLElement;
  img: HTMLImageElement;
}

export async function mountPages(job: EditorJob, frame: Frame) {
  const path = job.paths[0];
  const done = loading(frame.body);
  const count = await api.pdfInfo(path);
  done();

  const grid = el("div", { class: "pages" });
  frame.body.append(grid);
  const cards: Card[] = [];

  const summary = () => {
    const kept = cards.filter((c) => !c.removed).length;
    const moved = [...grid.children].some((n, i) => cards.find((c) => c.node === n)!.index !== i);
    const turned = cards.filter((c) => c.rotate % 360 !== 0).length;
    const bits = [`${kept} of ${count} pages`];
    if (moved) bits.push("reordered");
    if (turned) bits.push(`${turned} rotated`);
    frame.status(`${bits.join(" · ")}. Drag pages to reorder.`);
  };

  for (let i = 0; i < count; i++) {
    const img = el("img", { alt: `Page ${i + 1}`, draggable: false });
    const rotate = el("button", { title: "Rotate" });
    rotate.innerHTML = icon("rotate", 14);
    const remove = el("button", { title: "Remove page" });
    remove.innerHTML = icon("trash", 14);
    const node = el(
      "div",
      { class: "page-card" },
      el("div", { class: "thumb" }, img),
      el("div", { class: "meta" }, el("span", { textContent: `Page ${i + 1}` }), rotate, remove),
    );
    const card: Card = { index: i, rotate: 0, removed: false, node, img };
    cards.push(card);
    grid.append(node);

    rotate.addEventListener("click", (e) => {
      e.stopPropagation();
      card.rotate = (card.rotate + 90) % 360;
      img.style.transform = `rotate(${card.rotate}deg)`;
      summary();
    });
    remove.addEventListener("click", (e) => {
      e.stopPropagation();
      card.removed = !card.removed;
      node.classList.toggle("removed", card.removed);
      remove.innerHTML = icon(card.removed ? "undo" : "trash", 14);
      remove.title = card.removed ? "Keep page" : "Remove page";
      summary();
    });
    node.addEventListener("pointerdown", (e) => startDrag(e, node));
  }
  summary();

  // Thumbnails load one by one in the background.
  (async () => {
    for (const card of cards) {
      card.img.src = await api.pdfPage(path, card.index, 220).catch(() => "");
    }
  })();

  function startDrag(e: PointerEvent, node: HTMLElement) {
    if ((e.target as HTMLElement).closest("button")) return;
    e.preventDefault();
    node.setPointerCapture(e.pointerId);
    node.classList.add("dragging");
    const move = (ev: PointerEvent) => {
      node.style.pointerEvents = "none";
      const over = document.elementFromPoint(ev.clientX, ev.clientY)?.closest<HTMLElement>(".page-card");
      node.style.pointerEvents = "";
      if (!over || over === node) return;
      const r = over.getBoundingClientRect();
      const after = ev.clientX > r.left + r.width / 2;
      grid.insertBefore(node, after ? over.nextSibling : over);
    };
    const up = () => {
      node.classList.remove("dragging");
      node.removeEventListener("pointermove", move);
      node.removeEventListener("pointerup", up);
      summary();
    };
    node.addEventListener("pointermove", move);
    node.addEventListener("pointerup", up);
  }

  frame.apply.textContent = "Save copy";
  frame.apply.addEventListener("click", () => {
    const pages = [...grid.children]
      .map((n) => cards.find((c) => c.node === n)!)
      .filter((c) => !c.removed)
      .map((c) => ({ index: c.index, rotate: c.rotate }));
    if (!pages.length) {
      frame.status("Keep at least one page", true);
      return;
    }
    frame.submit({ pages });
  });
}
