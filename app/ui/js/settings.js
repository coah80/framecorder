// settings: the frames paired, where clips go, how syncing works.

import { $, ui, el, android, invoke, toast } from "./state.js";

const STATE_TEXT = {
  connected: "connected",
  connecting: "looking for it...",
  unreachable: "can't reach it",
  unpaired: "doesn't know this device anymore",
  wrong_fingerprint: "isn't the frame we paired with",
  full: "this device is out of space",
};

// asks once more before doing it: the first click arms, the second goes
function confirmed(button, label, action) {
  let armed;
  button.onclick = async () => {
    if (!armed) {
      button.textContent = "sure?";
      button.classList.add("danger");
      armed = setTimeout(() => {
        armed = null;
        button.textContent = label;
        button.classList.remove("danger");
      }, 3000);
      return;
    }
    clearTimeout(armed);
    await action();
  };
}

export function renderSettings(refresh) {
  $("paired").replaceChildren(
    ...ui.hosts.map((h) => {
      const line = el("div", "row setting");
      const who = el("div", "who");
      const text = el("div");
      text.append(el("b", null, h.name), el("p", "muted small", STATE_TEXT[h.state] || h.state));
      const dot = el("span", "dot");
      if (h.state === "connected") dot.style.background = "var(--green)";
      else if (h.state !== "connecting") dot.style.background = "var(--red)";
      who.append(dot, text);

      const unpair = el("button", "btn small", "unpair");
      unpair.title = "clips you already have stay where they are";
      confirmed(unpair, "unpair", async () => {
        await invoke("unpair", { fingerprint: h.fingerprint }).catch(toast);
        await refresh();
      });
      line.append(who, unpair);
      return line;
    }),
  );
  $("autostart-row").hidden = ui.autostart == null;
  $("autostart").checked = !!ui.autostart;
  renderBackground();
  $("where").textContent = ui.downloadDir;
  $("open-folder-2").hidden = android();
}

// desktop only: whether it stays in the tray, and leaving it
function renderBackground() {
  const desktop = ui.background != null;
  $("background-row").hidden = !desktop;
  $("quit-row").hidden = !desktop;
  $("background").checked = !!ui.background;
  const tray = ui.platform === "macos" ? "menu bar" : "tray";
  $("autostart-what").textContent = ui.background
    ? `sits in the ${tray}, so clips arrive without opening anything`
    : "opens when you log in, so clips arrive without you opening it";
  $("background-what").textContent = ui.background
    ? `closing the window leaves it syncing in the ${tray}`
    : "closing the window quits it. clips catch up the next time it's open";
}

export function initSettings() {
  const open = () => invoke("open_folder").catch(toast);
  $("open-folder").onclick = open;
  $("open-folder-2").onclick = open;
  $("background").addEventListener("change", async (e) => {
    try {
      ui.background = await invoke("set_background", { enabled: e.target.checked });
    } catch (err) {
      e.target.checked = !e.target.checked;
      toast(err);
    }
    renderBackground();
  });
  $("quit").onclick = () => invoke("quit").catch(toast);
  $("autostart").addEventListener("change", async (e) => {
    try {
      e.target.checked = await invoke("set_autostart", { enabled: e.target.checked });
    } catch (err) {
      e.target.checked = !e.target.checked;
      toast(err);
    }
  });
}
