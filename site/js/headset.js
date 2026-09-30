// the steam frame in the hero, rebuilt from a 360 of the reference model.
// model/steam-frame.bin is a compact pack of it (see pack_web.py next to the
// model sources): quantized positions, normals and a grey per vertex, split
// into material groups so each part gets its own finish here.

import * as THREE from "../vendor/three.module.min.js";

/// How wide it ends up, in scene units.
const WIDTH = 4.3;

const FINISH = {
  glass: () => new THREE.MeshPhysicalMaterial({ color: 0x0c0c0e, roughness: 0.3, clearcoat: 0.6, clearcoatRoughness: 0.28 }),
  shell: () => new THREE.MeshStandardMaterial({ color: 0x141416, roughness: 0.55, metalness: 0.05 }),
  fabric: () => new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 1.0 }),
  webbing: () => new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.8 }),
  lens: () => new THREE.MeshPhysicalMaterial({ color: 0x2a0f4a, roughness: 0.1, metalness: 0.6, clearcoat: 1.0 }),
  interior: () => new THREE.MeshStandardMaterial({ color: 0x08080a, roughness: 0.8 }),
  button: () => new THREE.MeshStandardMaterial({ color: 0x18181a, roughness: 0.45 }),
  led: () => new THREE.MeshStandardMaterial({ color: 0xffffff, emissive: 0xffffff, emissiveIntensity: 0.6 }),
};

async function fetchPack(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`couldn't load the headset (${res.status})`);
  return res.arrayBuffer();
}

function part(buffer, at, group, lo, hi) {
  const { verts, tris } = group;
  const q = new Int16Array(buffer, at, verts * 3);
  const n = new Int8Array(buffer, at + verts * 6, verts * 3);
  const g = new Uint8Array(buffer, at + verts * 9, verts);
  const indexAt = at + verts * 10 + ((4 - ((verts * 10) % 4)) % 4);
  const index = new Uint32Array(buffer, indexAt, tris * 3);

  const position = new Float32Array(verts * 3);
  const normal = new Float32Array(verts * 3);
  const color = new Float32Array(verts * 3);
  const linear = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  for (let i = 0; i < verts * 3; i++) {
    const k = i % 3;
    position[i] = lo[k] + ((q[i] + 32768) / 65535) * (hi[k] - lo[k]);
    normal[i] = n[i] / 127;
    color[i] = linear(g[(i / 3) | 0] / 255); // the pack stores sRGB greys
  }
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(position, 3));
  geometry.setAttribute("normal", new THREE.BufferAttribute(normal, 3));
  geometry.setAttribute("color", new THREE.BufferAttribute(color, 3));
  geometry.setIndex(new THREE.BufferAttribute(index, 1));
  const make = FINISH[group.name] ?? FINISH.shell;
  return { mesh: new THREE.Mesh(geometry, make()), next: indexAt + tris * 12 };
}

export async function headset() {
  const buffer = await fetchPack("model/steam-frame.bin");
  const headLength = new DataView(buffer).getUint32(0, true);
  const head = JSON.parse(new TextDecoder().decode(new Uint8Array(buffer, 4, headLength)));

  const object = new THREE.Group();
  let at = 4 + headLength;
  for (const group of head.groups) {
    const { mesh, next } = part(buffer, at, group, head.lo, head.hi);
    object.add(mesh);
    at = next;
  }

  const box = new THREE.Box3().setFromObject(object);
  object.position.sub(box.getCenter(new THREE.Vector3()));
  const centred = new THREE.Group();
  centred.add(object);
  centred.scale.setScalar(WIDTH / (box.max.x - box.min.x));
  return centred;
}
