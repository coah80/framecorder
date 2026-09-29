// framecorder app UI. plain js on purpose: no build step, tiny bundle.

import { $, ui, android, invoke, listen } from "./state.js";
import { renderStatus, renderProgress, renderNeeds } from "./status.js";
import { renderGallery, initGallery } from "./gallery.js";
import { initPairing, startPairing, stopPairing } from "./pairing.js";
import { renderSettings, initSettings } from "./settings.js";

function showPairing(on) {
  ui.pairing = on;
  render();
  if (on) startPairing();
  else stopPairing();
}

function show(view) {
  ui.view = view;
  ui.pairing = false;
  stopPairing();
  render();
  window.scrollTo(0, 0);
}

function render() {
  const paired = ui.hosts.length > 0;
  const pairing = ui.pairing || !paired;
  document.body.classList.toggle("android", android());
  $("tabs").hidden = !paired;
  $("view-pair").hidden = !pairing;
  $("view-clips").hidden = pairing || ui.view !== "clips";
  $("view-settings").hidden = pairing || ui.view !== "settings";
  $("pair-cancel").hidden = !paired;
  $("scan").hidden = !android();
  $("open-folder").hidden = android();
  for (const t of document.querySelectorAll(".tab")) {
    if (!pairing && t.dataset.view === ui.view) t.setAttribute("aria-current", "page");
    else t.removeAttribute("aria-current");
  }
  renderNeeds();
  renderStatus(showPairing);
  renderProgress();
  renderSettings(refresh);
}

async function refresh() {
  const o = await invoke("overview");
  ui.platform = o.platform;
  document.getElementById("tray-word").textContent = o.platform === "macos" ? "menu bar" : "tray";
  ui.hosts = o.hosts;
  ui.clips = o.clips;
  ui.downloadDir = o.download_dir;
  ui.autostart = o.autostart;
  // nothing paired (yet, or anymore): pairing is all there is to do
  const start = !ui.hosts.length && !ui.pairing;
  if (start) ui.pairing = true;
  render();
  renderGallery();
  if (start) startPairing();
}

listen("status", ({ payload }) => {
  const i = ui.hosts.findIndex((h) => h.fingerprint === payload.fingerprint);
  if (i >= 0) ui.hosts[i] = payload;
  else ui.hosts.push(payload);
  renderStatus(showPairing);
  renderSettings(refresh);
});

listen("progress", ({ payload }) => {
  ui.progress = payload;
  ui.busy = true;
  renderProgress();
  renderStatus(showPairing);
});

listen("busy", ({ payload }) => {
  ui.busy = payload;
  if (!payload) ui.progress = null;
  renderProgress();
  renderStatus(showPairing);
});

listen("synced", ({ payload }) => {
  const fresh = !ui.clips.some((c) => c.key === payload.key);
  ui.clips = [payload, ...ui.clips.filter((c) => c.key !== payload.key)];
  renderGallery(fresh ? payload.key : null);
});

for (const t of document.querySelectorAll(".tab")) t.onclick = () => show(t.dataset.view);
$("pair-cancel").onclick = () => showPairing(false);
$("pair-another").onclick = () => showPairing(true);

initGallery();
initSettings();
initPairing(async () => {
  ui.view = "clips";
  showPairing(false);
  await refresh();
});

refresh()
  .then(() => invoke("app_ready"))
  .catch((e) => console.error(e));
