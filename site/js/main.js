// the hero: a headset turning slowly, with what it's recording going
// round it. everything else on the page is plain html.

import * as THREE from "../vendor/three.module.min.js";
import { headset } from "./headset.js";

// where the project lives. the one thing to change before publishing
const REPO = "https://github.com/coah80/framecorder";
// what frame drop installs: packaging/release.sh makes it, a release hosts it
const MANIFEST = `${REPO}/releases/latest/download/framecorder.framedrop.json`;

const $ = (id) => document.getElementById(id);
const still = matchMedia("(prefers-reduced-motion: reduce)").matches || new URLSearchParams(location.search).has("still");

// ?still holds everything in place, for taking screenshots of the page
if (new URLSearchParams(location.search).has("still")) document.documentElement.classList.add("still");

for (const a of document.querySelectorAll('[data-link="repo"]')) a.href = REPO;
$("install").href = `https://framedropvr.com/install?manifest=${encodeURIComponent(MANIFEST)}`;

// the sync app: every download comes from the latest release, and the one
// for the device you're on is the filled in button
const agent = navigator.userAgent.toLowerCase();
const here = agent.includes("android") ? "android" : agent.includes("windows") ? "windows" : agent.includes("mac") ? "macos" : "linux";
for (const app of $("apps").children) {
  if (app.dataset.file) app.href = `${REPO}/releases/latest/download/${app.dataset.file}`;
  if (app.dataset.os === here && app.dataset.file) app.classList.remove("plain");
}
for (const s of document.querySelectorAll('[data-text="repo"]')) s.textContent = REPO;

// macs block the app the first time, so if you're on one the how-to starts open
if (here === "macos") $("mac").open = true;

// every copy button copies the commands right before it
for (const copy of document.querySelectorAll(".copy")) copy.onclick = async () => {
  await navigator.clipboard.writeText(copy.previousElementSibling.textContent.trim());
  copy.textContent = "copied";
  copy.classList.add("done");
  setTimeout(() => {
    copy.textContent = "copy";
    copy.classList.remove("done");
  }, 1600);
};

/// How fast things go round the headset, in radians a second.
const SPEED = 0.28;
/// How the headset is turned and tipped, in radians.
const POSE = -0.62;
const LEAN = 0.16;
/// The camera's view, top to bottom, in degrees. On a phone the stage is
/// wider than it is tall and the css says so, so the same view goes side to side.
const FOV = 38;
/// How big the floating tags get when they're nearest.
const NEAREST = 1.1;

function clock(seconds) {
  const two = (n) => String(Math.floor(n)).padStart(2, "0");
  return `${two(seconds / 3600)}:${two((seconds / 60) % 60)}:${two(seconds % 60)}`;
}

async function scene(stage, canvas) {
  const renderer = new THREE.WebGLRenderer({ canvas, antialias: true, alpha: true });
  renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
  renderer.toneMapping = THREE.ACESFilmicToneMapping;

  const world = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(FOV, 1, 0.1, 50);
  camera.position.set(0, 0.5, 9);
  camera.lookAt(0, 0, 0);

  // mauve from one side, cool from the other, so the edges read on dark
  world.add(new THREE.HemisphereLight(0xcdd6f4, 0x45475a, 2.6));
  const key = new THREE.DirectionalLight(0xffffff, 4.5);
  key.position.set(3, 4, 6);
  const fill = new THREE.DirectionalLight(0xcdd6f4, 2.2);
  fill.position.set(-5, 1, 4);
  const rim = new THREE.PointLight(0xcba6f7, 260, 24);
  rim.position.set(-4, 2, -3);
  const cool = new THREE.PointLight(0x89b4fa, 140, 24);
  cool.position.set(4, -2, 3);
  world.add(key, fill, rim, cool);

  // it sits still, turned a little so you see the front and one side
  const model = await headset();
  model.rotation.y = POSE;
  const tilt = new THREE.Group();
  tilt.rotation.set(LEAN, 0, 0);
  tilt.add(model);
  world.add(tilt);

  const orbits = [...stage.querySelectorAll(".orbit")].map((el) => ({
    el,
    radius: +el.dataset.radius,
    height: +el.dataset.height,
    phase: +el.dataset.phase,
  }));

  let size = 0;
  let now = 0;
  function resize() {
    size = stage.clientWidth;
    renderer.setSize(size, stage.clientHeight, false);
    camera.aspect = size / stage.clientHeight;
    const fitWidth = getComputedStyle(stage).getPropertyValue("--fit").trim() === "width";
    camera.fov = fitWidth ? THREE.MathUtils.radToDeg(2 * Math.atan(Math.tan(THREE.MathUtils.degToRad(FOV / 2)) / camera.aspect)) : FOV;
    camera.updateProjectionMatrix();
    // resizing wipes the canvas, so draw it again right away
    draw(now);
  }

  // with a mouse it leans towards the pointer. on a touch screen a finger is
  // a scroll, not a pointer, so it drifts on its own instead
  const pointer = { x: 0, y: 0 };
  let idle = matchMedia("(hover: none) and (pointer: coarse)").matches;
  addEventListener("pointermove", (e) => {
    if (e.pointerType !== "mouse") return;
    idle = false;
    pointer.x = (e.clientX / innerWidth - 0.5) * 2;
    pointer.y = (e.clientY / innerHeight - 0.5) * 2;
  });

  const at = new THREE.Vector3();
  function draw(t) {
    now = t;
    if (idle) {
      pointer.x = Math.sin(t * 0.31) * 0.45;
      pointer.y = Math.sin(t * 0.19) * 0.3;
    }
    // no spinning: it only leans a little towards the pointer
    tilt.rotation.x += (LEAN + pointer.y * 0.1 - tilt.rotation.x) * 0.05;
    tilt.rotation.y += (pointer.x * 0.18 - tilt.rotation.y) * 0.05;
    renderer.render(world, camera);

    $("timer").textContent = clock(754 + t);
    // how far the view reaches to each side, and how far away the camera is
    const reach = Math.tan(THREE.MathUtils.degToRad(camera.fov / 2)) * camera.aspect;
    const far = camera.position.length();
    for (const o of orbits) {
      const a = o.phase - t * SPEED;
      // the circle it goes round is made small enough that its widest point
      // still fits in the stage, so nothing drifts over the words next to it.
      // r / sqrt(d² - r²) is how far out a circle of r looks from d away
      const room = Math.max(1 - (o.el.offsetWidth * NEAREST + 12) / size, 0.1) * reach;
      const radius = Math.min(o.radius, (far * room) / Math.sqrt(1 + room * room));
      at.set(Math.sin(a) * radius, o.height + Math.sin(t * 0.8 + o.phase) * 0.08, Math.cos(a) * radius);
      const depth = at.z / radius; // 1 nearest, -1 behind the headset
      at.project(camera);
      const x = (at.x * 0.5 + 0.5) * size;
      const y = (-at.y * 0.5 + 0.5) * stage.clientHeight;
      const scale = NEAREST - (1 - depth) * 0.16;
      o.el.style.transform = `translate(${x}px, ${y}px) translate(-50%, -50%) scale(${scale})`;
      o.el.style.opacity = 0.35 + (depth + 1) * 0.325;
      o.el.style.zIndex = depth > 0 ? 3 : 1;
    }
  }
  // it only draws while it's on screen in a tab you're looking at
  let raf = 0;
  let seen = true;
  function frame(ms) {
    draw(ms / 1000);
    raf = requestAnimationFrame(frame);
  }
  function wake() {
    const on = !still && seen && !document.hidden;
    if (on && !raf) raf = requestAnimationFrame(frame);
    if (!on && raf) raf = (cancelAnimationFrame(raf), 0);
  }
  new IntersectionObserver(([entry]) => {
    seen = entry.isIntersecting;
    wake();
  }).observe(stage);
  document.addEventListener("visibilitychange", wake);
  new ResizeObserver(resize).observe(stage);
  resize();
  if (still) draw(2.2);
  else wake();
}

// no webgl, or the model didn't load: the page still says everything it needs to
scene($("stage"), $("scene")).catch((e) => {
  console.warn("no 3d here:", e);
  $("stage").classList.add("flat");
});
