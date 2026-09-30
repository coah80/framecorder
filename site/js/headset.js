// the steam frame in the hero. the model is "steam frame low poly" by
// jwwwun on sketchfab, with the stand it came on taken off.

import * as THREE from "../vendor/three.module.min.js";
import { OBJLoader } from "../vendor/OBJLoader.js";

/// How wide it ends up, in scene units.
const WIDTH = 4.3;

function load(loader, url) {
  return new Promise((done, failed) => loader.load(url, done, undefined, failed));
}

export async function headset() {
  const [object, map] = await Promise.all([
    load(new OBJLoader(), "model/steam-frame.obj"),
    load(new THREE.TextureLoader(), "model/texture.webp"),
  ]);
  map.colorSpace = THREE.SRGBColorSpace;
  map.anisotropy = 8;

  // the headset is black on a dark page: a little gloss so the lights can
  // pick out its edges
  const material = new THREE.MeshStandardMaterial({ map, roughness: 0.42, metalness: 0.15, side: THREE.DoubleSide });
  object.traverse((part) => {
    if (part.isMesh) part.material = material;
  });

  const box = new THREE.Box3().setFromObject(object);
  object.position.sub(box.getCenter(new THREE.Vector3()));
  const centred = new THREE.Group();
  centred.add(object);
  centred.scale.setScalar(WIDTH / (box.max.x - box.min.x));
  return centred;
}
