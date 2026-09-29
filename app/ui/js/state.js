// what the app knows right now, shared by every screen.

export const { invoke, convertFileSrc } = window.__TAURI__.core;
export const { listen } = window.__TAURI__.event;
export const $ = (id) => document.getElementById(id);

export const ui = {
  platform: "linux",
  hosts: [],
  clips: [],
  downloadDir: "",
  autostart: null,
  view: "clips", // clips | settings
  pairing: false,
  filter: "all", // all | clip | recording
  progress: null,
  busy: false,
};

export const android = () => ui.platform === "android";

export function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text != null) node.textContent = text;
  return node;
}

// a copy of one of the icons in index.html, by the button it sits in
export function icon(path) {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("aria-hidden", "true");
  svg.innerHTML = path;
  return svg;
}

let toastTimer;
export function toast(message) {
  const t = $("toast");
  t.textContent = String(message);
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.hidden = true), 5000);
}
