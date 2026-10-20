import { api, el, EditorJob, fileName } from "../shared/api";

export interface Frame {
  toolbar: HTMLElement;
  body: HTMLElement;
  apply: HTMLButtonElement;
  status(text: string, error?: boolean): void;
  submit(params: unknown): Promise<void>;
}

/** Header, optional toolbar, scrolling body and an Apply/Cancel footer. */
export function createFrame(root: HTMLElement, job: EditorJob, applyLabel = "Apply"): Frame {
  const names = job.paths.map(fileName);
  const head = el(
    "header",
    { class: "ed-head" },
    el("h1", { textContent: job.item.label }),
    el("div", {
      class: "file",
      textContent: names.length > 1 ? `${names[0]} and ${names.length - 1} more` : names[0],
    }),
  );
  const toolbar = el("div", { class: "ed-toolbar" });
  toolbar.hidden = true;
  const body = el("div", { class: "ed-body" });
  const statusEl = el("div", { class: "status" });
  const cancel = el("button", { class: "btn", textContent: "Cancel" });
  const apply = el("button", { class: "btn primary", textContent: applyLabel });
  const foot = el("footer", { class: "ed-foot" }, statusEl, cancel, apply);
  root.append(head, toolbar, body, foot);

  cancel.addEventListener("click", () => api.editorCancel());
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !(e.target instanceof HTMLInputElement)) api.editorCancel();
  });

  return {
    toolbar,
    body,
    apply,
    status(text, error = false) {
      statusEl.textContent = text;
      statusEl.classList.toggle("error", error);
    },
    async submit(params) {
      apply.disabled = true;
      try {
        await api.editorSubmit(params);
      } catch (err) {
        apply.disabled = false;
        this.status(String(err), true);
      }
    },
  };
}

export function segmented<T extends string>(
  options: { value: T; label: string; html?: string; title?: string }[],
  initial: T,
  onChange: (value: T) => void,
): HTMLElement {
  const wrap = el("div", { class: "seg" });
  for (const opt of options) {
    const b = el("button", { title: opt.title ?? opt.label });
    if (opt.html) b.innerHTML = opt.html;
    else b.textContent = opt.label;
    if (opt.value === initial) b.classList.add("on");
    b.addEventListener("click", () => {
      wrap.querySelectorAll("button").forEach((x) => x.classList.remove("on"));
      b.classList.add("on");
      onChange(opt.value);
    });
    wrap.append(b);
  }
  return wrap;
}

export function loading(body: HTMLElement, text = "Loading…"): () => void {
  const node = el("div", { class: "loading", textContent: text });
  body.append(node);
  return () => node.remove();
}
