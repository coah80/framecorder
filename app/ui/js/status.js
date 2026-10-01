// how each frame is doing: a card per frame.

import { $, ui, el, android, invoke, toast } from "./state.js";

const NEEDS = ["your frame is on", "it's on this wi-fi", "framecorder is running on it"];

// what happens to clips saved while the app isn't running, in settings
export function renderNeeds() {
  $("how-closed").textContent = android()
    ? "if the app is closed or android stops it, clips catch up the next time it's open."
    : "if the app is closed (not just the window), clips catch up the next time it's open.";
}

function ok(h) {
  const card = el("div", "card frame");
  card.dataset.state = h.state;
  const text = el("div", "grow");
  const title = el("div", "title");
  title.append(el("h3", null, h.name));
  text.append(
    title,
    el("p", "state", h.state === "connected" ? "connected. new clips land here on their own" : "looking for it on this network..."),
  );
  card.append(el("span", "dot"), text);
  const u = h.update;
  if (u && (u.available || u.updating)) {
    title.append(el("span", "pill", u.updating ? "updating..." : "new update!"));
    if (u.updating) {
      text.append(el("p", "detail", `installing framecorder ${u.latest ?? "update"} on ${h.name}. it reconnects on its own when it's done.`));
    } else {
      text.append(el("p", "detail", u.latest ? `framecorder ${u.latest} is out, ${h.name} has ${u.installed}.` : `there's a newer framecorder than ${h.name} has.`));
      const button = el("button", "btn primary", "update framecorder");
      button.title = "the frame installs it on its own, it would within a few hours anyway";
      button.onclick = async () => {
        button.disabled = true;
        button.textContent = "starting...";
        try {
          await invoke("start_update", { fingerprint: h.fingerprint });
        } catch (e) {
          toast(e);
          button.disabled = false;
          button.textContent = "update framecorder";
        }
      };
      card.append(button);
    }
  }
  return card;
}

function problem(h, showPairing) {
  const unreachable = h.state === "unreachable";
  const card = el("div", `card frame ${unreachable || h.state === "full" ? "bad" : "warn"}`);
  const text = el("div", "grow");
  const button = el("button", "btn");
  if (h.state === "full") {
    text.append(
      el("h3", null, android() ? "this phone is out of space" : "this computer is out of space"),
      el("p", "state", `the clips are still on ${h.name}, nothing is lost. free up some space and they sync on their own.`),
    );
    if (h.message) text.append(el("p", "detail", `the next one ${h.message}`));
    button.textContent = "try again";
    button.onclick = () => invoke("retry_now");
  } else if (unreachable) {
    const checks = el("ul", "checks");
    checks.append(...NEEDS.map((n) => el("li", null, n)));
    text.append(el("h3", null, `can't reach ${h.name}`), el("p", "state", "check that:"), checks);
    if (h.message) text.append(el("p", "detail", h.message));
    button.textContent = "try again";
    button.onclick = () => invoke("retry_now");
  } else {
    const forgot = h.state === "unpaired";
    text.append(
      el("h3", null, forgot ? `${h.name} forgot this device` : `that isn't ${h.name}`),
      el("p", "state", forgot
        ? "it was removed on the frame. pair again to keep syncing."
        : "something else answered where your frame was, so we're not talking to it."),
    );
    button.textContent = "pair again";
    button.onclick = () => showPairing(true);
  }
  card.append(el("span", "dot"), text, button);
  return card;
}

export function renderStatus(showPairing) {
  const fine = (h) => h.state === "connected" || h.state === "connecting";
  $("frames").replaceChildren(...ui.hosts.map((h) => (fine(h) ? ok(h) : problem(h, showPairing))));
}

export function renderProgress() {
  const p = ui.progress;
  $("progress").hidden = !p || !ui.busy;
  if (!p) return;
  const pct = p.total ? Math.floor((p.done / p.total) * 100) : 100;
  $("progress-name").textContent = `getting ${p.name}`;
  const more = p.queued ? ` · ${p.queued} more` : "";
  $("progress-text").textContent = `${pct}%${more}`;
  $("progress-bar").style.width = `${pct}%`;
}
