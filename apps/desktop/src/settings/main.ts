import "./settings.css";
import { open } from "@tauri-apps/plugin-dialog";
import { api, el, KEY_LABEL, Modifier, Settings } from "../shared/api";
import { icon } from "../shared/icons";

const root = document.getElementById("app")!;
const MODIFIERS: Modifier[] = ["Shift", "Control", "Space", "Alt", "Super"];

function toggle(checked: boolean, onChange: (v: boolean) => void): HTMLElement {
  const input = el("input", { type: "checkbox", checked });
  input.addEventListener("change", () => onChange(input.checked));
  return el("label", { class: "switch" }, input, el("span"));
}

function row(title: string, detail: string | Node, control: Node): HTMLElement {
  const small = el("small");
  small.append(detail);
  return el("div", { class: "row" }, el("div", { class: "text" }, el("b", { textContent: title }), small), control);
}

function section(title: string, ...rows: Node[]): HTMLElement {
  return el("section", { class: "glass" }, el("h2", { textContent: title }), ...rows);
}

async function main() {
  let s = await api.settings();
  const session = await api.session();
  const save = async (patch: Partial<Settings>) => {
    s = await api.saveSettings({ ...s, ...patch });
    renderStatus();
  };

  // Header
  document.body.prepend(el("div", { class: "backdrop" }));
  const pill = el("div", { class: "status-pill glass" });
  const renderStatus = () => {
    pill.className = "status-pill glass";
    if (!session.dragWheel) {
      pill.textContent = "Drag wheel needs X11";
      pill.classList.add("off");
    } else if (s.paused) {
      pill.textContent = "Paused";
      pill.classList.add("paused");
    } else {
      pill.textContent = "Ready";
    }
  };
  renderStatus();
  // Only now: save() calls renderStatus, which must exist first.
  if (!s.onboarded) await save({ onboarded: true });
  root.append(
    el(
      "div",
      { class: "top" },
      Object.assign(el("div", { class: "logo" }), { innerHTML: icon("kumquat", 24) }),
      el("div", {}, el("h1", { textContent: "Kumquat" }), el("p", { textContent: "Convert and edit files right where they are." })),
      pill,
    ),
  );

  // How it works
  const key = (m: Modifier) => KEY_LABEL[m];
  const how = el("div", { class: "how" });
  const renderHow = () => {
    how.innerHTML = "";
    const steps: [string, string][] = [
      ["Start dragging a file", `From ${session.fileManager}, the desktop or any app that drags files.`],
      [
        `Tap ${key(s.convertKey)}`,
        `A wheel of formats appears. ${key(s.toolsKey)} shows tools, ${key(s.actionsKey)} shows actions.`,
      ],
      ["Drop it on a choice", "The result lands where you were dragging to: the desktop spot or the open folder."],
    ];
    steps.forEach(([b, t], i) =>
      how.append(el("div", { class: "glass" }, el("div", { class: "num", textContent: `0${i + 1}` }), el("b", { textContent: b }), el("span", { textContent: t }))),
    );
  };
  renderHow();
  root.append(how);

  // Keys
  const keySelect = (field: "convertKey" | "toolsKey" | "actionsKey", label: string) => {
    const select = el("select");
    for (const m of MODIFIERS) select.append(el("option", { value: m, textContent: KEY_LABEL[m], selected: s[field] === m }));
    select.addEventListener("change", async () => {
      const value = select.value as Modifier;
      // Swap with whichever wheel already used that key.
      const clash = (["convertKey", "toolsKey", "actionsKey"] as const).find((f) => f !== field && s[f] === value);
      const patch: Partial<Settings> = { [field]: value };
      if (clash) patch[clash] = s[field];
      await save(patch);
      renderKeys();
      renderHow();
    });
    return el("label", {}, label, select);
  };
  const keys = el("div", { class: "keys" });
  const renderKeys = () => {
    keys.innerHTML = "";
    keys.append(keySelect("convertKey", "Convert"), keySelect("toolsKey", "Tools"), keySelect("actionsKey", "Actions"));
  };
  renderKeys();

  const integration = await api.integrationStatus();
  const integrationDetail = integration.hint;

  const autostart = await api.autostart();

  root.append(
    section(
      "Wheel",
      row("Keys", "Tap one of these while dragging a file.", keys),
      row(
        "Drag wheel",
        session.dragWheel
          ? "Turn off to drag files normally, even with a key held."
          : "Your session is Wayland, where apps can't watch drags. Use the file manager shortcut below.",
        toggle(!s.paused, (on) => save({ paused: !on })),
      ),
      row(
        "File manager shortcut",
        integrationDetail,
        toggle(integration.installed, async (on) => {
          await api.setIntegration(on);
        }),
      ),
    ),
  );

  // Output
  const folderBtn = el("button", { class: "btn", textContent: s.outputFolder ? shortPath(s.outputFolder) : "Choose…" });
  folderBtn.addEventListener("click", async () => {
    const picked = await open({ directory: true, title: "Save results in" });
    if (typeof picked === "string") {
      await save({ outputFolder: picked, outputBesideOriginal: false });
      folderBtn.textContent = shortPath(picked);
      besideToggle.querySelector("input")!.checked = false;
    }
  });
  const besideToggle = toggle(s.outputBesideOriginal, (on) => save({ outputBesideOriginal: on || !s.outputFolder }));
  root.append(
    section(
      "Results",
      row(
        "Save where you drop",
        `Over the desktop or a ${session.fileManager} window, results land right there. Otherwise next to the original. Nothing is ever overwritten.`,
        besideToggle,
      ),
      row("Or always save in", "Used instead when the switch above is off.", folderBtn),
      row(`Show in ${session.fileManager} when done`, "Otherwise a notification lets you jump there.", toggle(s.revealAfter, (on) => save({ revealAfter: on }))),
      row("Notifications", "Progress for long jobs, and what finished or failed.", toggle(s.notifications, (on) => save({ notifications: on }))),
    ),
  );

  // Quality
  const slider = (min: number, max: number, value: number, fmt: (v: number) => string, onSet: (v: number) => void) => {
    const input = el("input", { type: "range", min: String(min), max: String(max), value: String(value) });
    const out = el("span", { class: "value", textContent: fmt(value) });
    input.addEventListener("input", () => (out.textContent = fmt(Number(input.value))));
    input.addEventListener("change", () => onSet(Number(input.value)));
    return el("div", { class: "keys" }, input, out);
  };
  const dpi = el("select");
  for (const v of [150, 200, 300, 600]) dpi.append(el("option", { value: String(v), textContent: `${v} DPI`, selected: s.pdfDpi === v }));
  dpi.addEventListener("change", () => save({ pdfDpi: Number(dpi.value) }));
  root.append(
    section(
      "Quality",
      row("Image quality", "For JPG, WebP, HEIC and AVIF.", slider(40, 100, s.imageQuality, (v) => `${v}`, (v) => save({ imageQuality: v }))),
      row(
        "Video quality",
        "Higher keeps more detail and makes bigger files.",
        slider(14, 32, 46 - s.videoCrf, (v) => `${Math.round(((v - 14) / 18) * 100)}%`, (v) => save({ videoCrf: 46 - v })),
      ),
      row("PDF pages as images", "Resolution when pages or slides become JPG or PNG.", dpi),
    ),
  );

  root.append(section("System", row("Start at login", "Keeps the wheel ready from the moment you sign in.", toggle(autostart, (on) => api.setAutostart(on)))));

  // Tools
  const tools = await api.toolStatus();
  const table = el("table", { class: "tools" });
  for (const t of tools) {
    const badge = el("span", {
      class: `badge ${t.path ? (t.bundled ? "bundled" : "system") : "missing"}`,
      textContent: t.path ? (t.bundled ? "bundled" : "system") : "missing",
    });
    table.append(
      el(
        "tr",
        {},
        el("td", { textContent: t.id }),
        el("td", {}, el("div", { textContent: t.purpose }), el("div", { class: "where", textContent: t.path ?? "Not found" })),
        el("td", {}, badge),
      ),
    );
  }
  const note = el("p", { class: "foot-note" });
  const fetchScript = session.platform === "windows" ? "scripts/fetch-binaries.ps1" : "scripts/fetch-binaries.sh";
  note.innerHTML = `Anything missing? Run <code>${fetchScript}</code> in the project to download bundled copies.`;
  root.append(el("section", { class: "glass" }, el("h2", { textContent: "Conversion engines" }), table, note));
}

function shortPath(p: string): string {
  const sep = p.includes("\\") ? "\\" : "/";
  const parts = p.split(sep).filter(Boolean);
  return parts.length > 2 ? `…${sep}${parts.slice(-2).join(sep)}` : p;
}

main().catch((err) => {
  root.append(el("p", { class: "foot-note", textContent: `Settings failed to load: ${err}` }));
});
