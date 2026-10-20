// Line icons on a 24×24 grid, drawn with `stroke="currentColor"`.
const PATHS: Record<string, string> = {
  kumquat:
    '<ellipse cx="11" cy="14" rx="7" ry="8"/><path d="M11 6c.4-1.9 1.8-3.2 3.8-3.6"/><path d="M14 4.6c2.6-2 5.8-1.2 6.6.2-2 2.2-4.9 2.4-6.6-.2Z"/>',
  compress:
    '<path d="M4 14h6v6"/><path d="M20 10h-6V4"/><path d="M14 10l7-7"/><path d="M3 21l7-7"/>',
  resize:
    '<path d="M15 3h6v6"/><path d="M9 21H3v-6"/><path d="M21 3l-7 7"/><path d="M3 21l7-7"/>',
  crop: '<path d="M6 2v14a2 2 0 0 0 2 2h14"/><path d="M18 22V8a2 2 0 0 0-2-2H2"/>',
  pen: '<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z"/>',
  rotate:
    '<path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 3v6h-6"/>',
  fill: '<path d="M19 11 11 3 2.5 11.5a2 2 0 0 0 0 2.8L7.7 19.5a2 2 0 0 0 2.8 0Z"/><path d="M5 11h14"/><path d="M22 20a2 2 0 1 1-4 0c0-1.6 2-4 2-4s2 2.4 2 4Z"/>',
  shield:
    '<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10Z"/><path d="m9 12 2 2 4-4"/>',
  qr: '<rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><path d="M14 14h3v3h-3zM20 14v.01M14 20h.01M17 20h4v-3"/>',
  merge:
    '<path d="m8 6 4-4 4 4"/><path d="M12 2v10.3a4 4 0 0 1-1.2 2.8L4 22"/><path d="m20 22-5-5"/>',
  split:
    '<path d="M16 3h5v5"/><path d="M8 3H3v5"/><path d="M12 22v-8.3a4 4 0 0 0-1.2-2.8L3 3"/><path d="m15 9 6-6"/>',
  pages:
    '<rect x="3" y="3" width="7" height="9" rx="1"/><rect x="14" y="3" width="7" height="5" rx="1"/><rect x="14" y="12" width="7" height="9" rx="1"/><rect x="3" y="16" width="7" height="5" rx="1"/>',
  redact:
    '<rect x="3" y="4" width="18" height="16" rx="2"/><rect x="6" y="8" width="8" height="3" fill="currentColor"/><path d="M6 15h12"/>',
  scissors:
    '<circle cx="6" cy="6" r="3"/><circle cx="6" cy="18" r="3"/><path d="M20 4 8.1 15.9M14.5 14.5 20 20M8.1 8.1 12 12"/>',
  speed: '<path d="M12 14l4-4"/><path d="M3.3 19a10 10 0 1 1 17.4 0"/>',
  music:
    '<path d="M9 18V5l12-2v13"/><circle cx="6" cy="18" r="3"/><circle cx="18" cy="16" r="3"/>',
  mute: '<path d="M11 5 6 9H2v6h4l5 4Z"/><path d="m22 9-6 6M16 9l6 6"/>',
  sparkle:
    '<path d="M12 3l1.9 5.1L19 10l-5.1 1.9L12 17l-1.9-5.1L5 10l5.1-1.9Z"/><path d="M19 17v4M17 19h4"/>',
  wave: '<path d="M2 12h2M6 8v8M10 4v16M14 7v10M18 10v4M22 12h0"/>',
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  extract:
    '<path d="M21 8v13H3V8"/><path d="M1 3h22v5H1z"/><path d="M10 12h4"/>',
  zip: '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z"/><path d="M10 6h1M10 9h1M10 12h1M10 15h1"/><rect x="9" y="17" width="3" height="3"/>',
  phone:
    '<rect x="6" y="2" width="12" height="20" rx="2"/><path d="M11 18h2"/>',
  pin: '<path d="M12 17v5"/><path d="M9 10.8V6h6v4.8l2.5 3.2h-11Z"/><path d="M8 2h8"/>',
  copy: '<rect x="8" y="8" width="13" height="13" rx="2"/><path d="M4 16a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h10a2 2 0 0 1 2 2"/>',
  move: '<path d="M4 20h16a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.7-.9l-.8-1.2A2 2 0 0 0 7.9 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/><path d="M12 10l3 3-3 3M8 13h7"/>',
  link: '<path d="M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7"/><path d="M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7"/>',
  terminal: '<path d="m4 17 6-6-6-6"/><path d="M12 19h8"/>',
  format:
    '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z"/><path d="M14 2v6h6"/>',
  close: '<path d="M18 6 6 18M6 6l12 12"/>',
  undo: '<path d="M3 7v6h6"/><path d="M21 17a9 9 0 0 0-15-6.7L3 13"/>',
  trash:
    '<path d="M3 6h18"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>',
  arrow: '<path d="M5 19 19 5"/><path d="M9 5h10v10"/>',
  rect: '<rect x="3" y="5" width="18" height="14" rx="1"/>',
  text: '<path d="M4 7V4h16v3"/><path d="M9 20h6"/><path d="M12 4v16"/>',
  highlighter:
    '<path d="m9 11-6 6v3h9l3-3"/><path d="m22 12-4.6 4.6a2 2 0 0 1-2.8 0l-5.2-5.2a2 2 0 0 1 0-2.8L14 4"/>',
  check: '<path d="M20 6 9 17l-5-5"/>',
  chevronLeft: '<path d="m15 18-6-6 6-6"/>',
  chevronRight: '<path d="m9 18 6-6-6-6"/>',
};

export function icon(name: string, size = 22): string {
  const body = PATHS[name] ?? PATHS.format;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${body}</svg>`;
}
