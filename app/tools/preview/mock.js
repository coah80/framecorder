// stands in for tauri so the ui runs in a plain browser.
// pick a state with ?s=connected|update|updating|syncing|unreachable|full|forgot|empty|pair|pair-found|settings and &p=android
(() => {
  const q = new URLSearchParams(location.search);
  const state = q.get("s") || "connected";
  const platform = q.get("p") || "windows";
  const listeners = {};
  const emit = (name, payload) => (listeners[name] || []).forEach((cb) => cb({ payload }));
  const now = Math.floor(Date.now() / 1000);
  const hour = 3600;

  const clip = (kind, ago, secs, mb, exists = true) => {
    const created = now - ago;
    const d = new Date(created * 1000);
    const pad = (n) => String(n).padStart(2, "0");
    const name = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}_${pad(d.getHours())}-${pad(d.getMinutes())}-${pad(d.getSeconds())}.mp4`;
    return { key: `${kind}-${created}`, kind, name, size: mb * 1e6, duration_s: secs, created, exists, location: `/nowhere/${name}` };
  };
  const clips = [
    clip("clip", 4 * 60, 30, 151), clip("clip", 22 * 60, 30, 148), clip("recording", 2 * hour, 754, 3771),
    clip("clip", 26 * hour, 15, 82.7), clip("recording", 27 * hour, 96, 463), clip("clip", 28 * hour, 16, 56.7, false),
    clip("recording", 5 * 24 * hour, 400, 1678),
  ];
  const host = (s, message = null, update = null) => ({ name: "frame", addr: "192.168.1.20:38619", fingerprint: "d2b7", state: s, message, update });
  const update = (updating) => ({ installed: "0.1.1", latest: "0.1.2", available: true, updating });
  const hosts = {
    connected: [host("connected")], syncing: [host("connected")], settings: [host("connected")], empty: [host("connected")],
    unreachable: [host("unreachable", "connection timed out")], forgot: [host("unpaired")],
    full: [host("full", "needs 4.0 GB free, there's 1.2 GB")],
    pair: [], "pair-found": [],
    update: [host("connected", null, update(false))], updating: [host("connected", null, update(true))],
  }[state];

  const commands = {
    overview: () => ({ platform, hosts, clips: state === "empty" || !hosts.length ? [] : clips, download_dir: platform === "android" ? "Movies/framecorder" : "C:\\Users\\you\\Videos\\framecorder", autostart: platform === "android" ? null : true, background: platform === "android" ? null : true }),
    app_ready: () => null,
    discover: () => new Promise((done) => {
      if (state === "pair-found") setTimeout(() => emit("discovered", { name: "frame", addr: "192.168.1.20:38619", fingerprint: "d2b7" }), 150);
      setTimeout(done, 400);
    }),
    pair: () => Promise.reject("that code isn't right, or it ran out. get a new one on the frame."),
    set_autostart: ({ enabled }) => enabled,
    set_background: ({ enabled }) => enabled,
    quit: () => null,
    start_update: () => null,
  };

  window.__TAURI__ = {
    core: {
      invoke: (cmd, args) => Promise.resolve().then(() => (commands[cmd] ? commands[cmd](args || {}) : null)),
      convertFileSrc: (p) => p,
    },
    event: { listen: (name, cb) => ((listeners[name] ||= []).push(cb), Promise.resolve(() => {})) },
  };

  // screenshots freeze animations wherever they happen to be, so: none
  if (q.has("still")) {
    const style = document.createElement("style");
    style.textContent = "*, *::before, *::after { animation: none !important; transition: none !important; }";
    document.documentElement.append(style);
  }

  window.addEventListener("load", () => setTimeout(() => {
    if (state === "syncing") {
      emit("busy", true);
      emit("progress", { name: "2026-09-29_15-04-11.mp4", done: 61e6, total: 151e6, queued: 2 });
    }
    if (state === "settings") document.querySelector('.tab[data-view="settings"]').click();
  }, 200));
})();
