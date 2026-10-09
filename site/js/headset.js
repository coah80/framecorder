// the steam frame in the hero. it's valve's own cad for the frame
// (gitlab.steamos.cloud/SteamHardware/SteamFrame, cc by-nc-sa 4.0), with
// the insides taken out: only what you can see from outside is in the glb.
// tools/frame_model.py is how it was made, and model/LICENSE.txt says what
// that licence asks of us.

import * as THREE from "../vendor/three.module.min.js";
import { GLTFLoader } from "../vendor/GLTFLoader.js";
import { MeshoptDecoder } from "../vendor/meshopt_decoder.module.js";

/// How wide it ends up, in scene units.
const WIDTH = 4.2;

// what each part is made of. parts are named after the solids in valve's cad
// (core is the headset, strap is the head strap). anything not listed is the
// dark plastic the rest of the shell is. the real thing is black and dark
// grey, so on a dark page it's the roughness that tells the parts apart
const LOOKS = {
  // the shell. a little gloss so the lights can pick out its edges
  plastic: { color: 0x1c1c22, roughness: 0.45, metalness: 0.1, parts: [] },
  // the front visor: glossy black, and the two lenses behind it
  glass: { color: 0x08080b, roughness: 0.14, metalness: 0.35, parts: ["core1", "core0", "core64", "core65"] },
  // the cameras and sensor windows
  camera: { color: 0x0c0c10, roughness: 0.25, metalness: 0.3, parts: ["core16", "core30", "core31", "core39", "core40", "core41", "core42", "core48"] },
  // the vent on top and the two under the sides
  grille: { color: 0x1a1a20, roughness: 0.7, metalness: 0.1, parts: ["core38", "core28", "core29"] },
  // the face gasket and its nose flap: foam under fabric
  foam: { color: 0x26262c, roughness: 0.95, metalness: 0, parts: ["strap4", "strap8", "strap5", "strap6"] },
  // the straps round the head
  fabric: { color: 0x4e4c50, roughness: 0.92, metalness: 0, parts: ["strap99", "strap100"] },
  // the cushion on the front of the rear pad (its back is plastic)
  cushion: { color: 0x303038, roughness: 0.9, metalness: 0, parts: ["strap0", "strap78", "strap79"] },
  // the cable from the headset to the rear pad
  cable: { color: 0x3a3a42, roughness: 0.6, metalness: 0.05, parts: ["strap7"] },
};

function load(loader, url) {
  return new Promise((done, failed) => loader.load(url, done, undefined, failed));
}

export async function headset() {
  const loader = new GLTFLoader();
  loader.setMeshoptDecoder(MeshoptDecoder);
  const { scene: object } = await load(loader, "model/steam-frame.glb");

  const materials = {};
  const lookOf = {};
  for (const [look, { parts, ...spec }] of Object.entries(LOOKS)) {
    materials[look] = new THREE.MeshStandardMaterial({ ...spec, side: THREE.DoubleSide });
    for (const part of parts) lookOf[part] = look;
  }
  object.traverse((part) => {
    if (!part.isMesh) return;
    // gltfpack keeps the part's name on the node above the mesh, and the
    // loader calls the mesh itself mesh_<n>
    const name = part.parent.name || part.name;
    part.material = materials[lookOf[name] ?? "plastic"];
  });

  const box = new THREE.Box3().setFromObject(object);
  object.position.sub(box.getCenter(new THREE.Vector3()));
  const centred = new THREE.Group();
  centred.add(object);
  centred.scale.setScalar(WIDTH / (box.max.x - box.min.x));
  return centred;
}
