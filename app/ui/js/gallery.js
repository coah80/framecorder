// the clips: newest first, a day at a time, one row each.

import { $, ui, el, icon, android, invoke, convertFileSrc, toast } from "./state.js";
import { size, length, time, day, dayKey } from "./format.js";

const PLAY = '<path d="m9 7 9 5-9 5z"/>';
const FOLDER = '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>';
const SHARE = '<path d="M12 15V4M8 8l4-4 4 4M5 13v5a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-5"/>';

// rows are kept between renders so thumbnails don't reload
const rows = new Map();

const thumbs = new IntersectionObserver((entries) => {
  for (const e of entries) {
    if (!e.isIntersecting) continue;
    thumbs.unobserve(e.target);
    const v = document.createElement("video");
    v.muted = true;
    v.preload = "metadata";
    v.playsInline = true;
    // a frame a second in, the very first is often black
    v.src = convertFileSrc(e.target.dataset.location) + "#t=1";
    v.addEventListener("loadeddata", () => v.classList.add("ready"), { once: true });
    v.addEventListener("error", () => v.remove(), { once: true });
    e.target.prepend(v);
  }
}, { rootMargin: "200px" });

function run(cmd, c) {
  return invoke(cmd, { key: c.key }).catch(toast);
}

function row(c, fresh) {
  const signature = `${c.exists}:${c.size}`;
  const kept = rows.get(c.key);
  if (kept && kept.dataset.signature === signature) return kept;

  const li = el("li", "clip");
  li.dataset.kind = c.kind;
  li.dataset.signature = signature;
  li.title = c.name;
  if (fresh) li.classList.add("fresh");

  const thumb = el("div", "thumb");
  thumb.append(icon(PLAY));
  if (c.duration_s != null) thumb.append(el("span", "len", length(c.duration_s)));

  const text = el("div", "clip-text");
  const sub = el("div", "sub");
  sub.append(el("span", "kind", c.kind === "clip" ? "clip" : "recording"));
  if (c.duration_s != null) sub.append(el("span", "data len-text", length(c.duration_s)));
  sub.append(el("span", "data", c.exists ? size(c.size) : "moved or deleted"));
  text.append(el("b", null, time(c.created)), sub);
  li.append(thumb, text);

  if (c.exists) {
    li.tabIndex = 0;
    li.setAttribute("role", "button");
    li.onclick = () => run("open_clip", c);
    li.onkeydown = (e) => (e.key === "Enter" || e.key === " ") && run("open_clip", c);

    const more = el("button", "btn icon");
    more.append(icon(android() ? SHARE : FOLDER));
    more.title = android() ? "share" : "show in folder";
    more.setAttribute("aria-label", more.title);
    more.onclick = (e) => {
      e.stopPropagation();
      run(android() ? "share_clip" : "reveal_clip", c);
    };
    li.append(more);

    if (!android()) {
      thumb.dataset.location = c.location;
      thumbs.observe(thumb);
    }
  } else {
    li.classList.add("missing");
  }
  rows.set(c.key, li);
  return li;
}

function counts() {
  const clips = ui.clips.filter((c) => c.kind === "clip").length;
  return { all: ui.clips.length, clip: clips, recording: ui.clips.length - clips };
}

export function renderGallery(freshKey) {
  const n = counts();
  for (const b of $("filter").querySelectorAll("button")) {
    b.setAttribute("aria-pressed", String(b.dataset.kind === ui.filter));
    b.querySelector(".n").textContent = n[b.dataset.kind] || "";
  }

  const shown = ui.clips
    .filter((c) => ui.filter === "all" || c.kind === ui.filter)
    .sort((a, b) => b.created - a.created);

  const empty = $("empty");
  empty.hidden = shown.length > 0;
  if (!ui.clips.length) empty.textContent = "nothing yet. save a clip or a recording on your frame and it shows up here.";
  else empty.textContent = ui.filter === "clip" ? "no clips yet, just recordings." : "no recordings yet, just clips.";

  const days = new Map();
  for (const c of shown) {
    const key = dayKey(c.created);
    if (!days.has(key)) days.set(key, []);
    days.get(key).push(c);
  }
  $("gallery").replaceChildren(
    ...[...days.values()].map((list) => {
      const group = el("section", "day");
      const ul = el("ul");
      ul.append(...list.map((c) => row(c, c.key === freshKey)));
      group.append(el("h4", null, day(list[0].created)), ul);
      return group;
    }),
  );
}

export function initGallery() {
  for (const b of $("filter").querySelectorAll("button")) {
    b.onclick = () => {
      ui.filter = b.dataset.kind;
      renderGallery();
    };
  }
}
