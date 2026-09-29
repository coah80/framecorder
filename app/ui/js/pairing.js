// pairing: find frames on this network, pick one, type its code.

import { $, ui, el, invoke, listen } from "./state.js";

// keeps looking while the pairing screen is open, so a frame that wakes up
// or joins the wi-fi late still shows up
const SCAN_EVERY_MS = 10_000;

const found = new Map();
let selected = null; // { addr, fingerprint }
let scanTimer;
let done = () => {};

function error(e) {
  $("pair-error").textContent = String(e);
  $("pair-error").hidden = false;
}

function renderFound() {
  $("found-empty").hidden = found.size > 0;
  // one frame around: that's the one
  if (!selected && found.size === 1) {
    const [only] = found.values();
    selected = { addr: only.addr, fingerprint: only.fingerprint };
  }
  $("pair-go").disabled = !selected;
  $("found").replaceChildren(
    ...[...found.values()].map((f) => {
      const picked = selected?.fingerprint === f.fingerprint;
      const b = el("button");
      b.type = "button";
      b.setAttribute("aria-pressed", String(picked));
      b.append(el("b", null, f.name), el("span", "muted small", picked ? "picked" : "on this network"));
      b.onclick = () => {
        selected = { addr: f.addr, fingerprint: f.fingerprint };
        renderFound();
        $("code").focus();
      };
      const li = el("li");
      li.append(b);
      return li;
    }),
  );
}

async function discover() {
  clearTimeout(scanTimer);
  renderFound();
  try {
    await invoke("discover", { seconds: 8 });
  } catch (e) {
    $("found-empty").textContent = `can't look for frames on this network (${e}).`;
    return;
  }
  if (!found.size) {
    $("found-empty").textContent = "none found yet. is it on, on this wi-fi, with framecorder running? still looking...";
  }
  if (ui.pairing) scanTimer = setTimeout(discover, SCAN_EVERY_MS);
}

export function startPairing() {
  $("pair-error").hidden = true;
  $("code").value = "";
  $("found-empty").textContent = "looking for frames on this network...";
  found.clear();
  selected = null;
  discover();
}

export function stopPairing() {
  clearTimeout(scanTimer);
}

async function pair(promise, button) {
  button.disabled = true;
  $("pair-error").hidden = true;
  try {
    await promise;
    await done();
  } catch (e) {
    error(e);
  } finally {
    button.disabled = !selected && button === $("pair-go");
  }
}

// `paired` runs once a frame has been paired
export function initPairing(paired) {
  done = paired;
  listen("discovered", ({ payload }) => {
    found.set(payload.fingerprint, payload);
    renderFound();
  });

  $("rescan").onclick = discover;
  $("code").addEventListener("input", (e) => {
    e.target.value = e.target.value.replace(/\D/g, "").slice(0, 6);
  });
  $("pair-form").addEventListener("submit", (e) => {
    e.preventDefault();
    if (!selected) return error("pick your frame in step 2 first");
    if ($("code").value.length < 6) return error("the code is 6 digits");
    pair(invoke("pair", { ...selected, code: $("code").value }), $("pair-go"));
  });
  $("scan").addEventListener("click", async () => {
    const scanner = window.__TAURI__.barcodeScanner;
    if (!scanner) return error("the scanner isn't available, type the code instead");
    try {
      let perm = await scanner.checkPermissions();
      if (perm !== "granted") perm = await scanner.requestPermissions();
      if (perm !== "granted") return error("framecorder needs the camera to scan. or type the code instead.");
      const res = await scanner.scan({ windowed: false, formats: [scanner.Format.QRCode] });
      await pair(invoke("pair_link", { link: res.content }), $("scan"));
    } catch (e) {
      error(e);
    }
  });
}
