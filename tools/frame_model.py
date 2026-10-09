"""The hero's steam frame, from valve's cad: the two step files to one small glb.

    python tools/frame_model.py core_module.stp head_strap.stp [out.glb]

Writes site/model/steam-frame.glb. Needs cadquery-ocp, trimesh and embreex (pip)
and gltfpack (npm i -g gltfpack). Takes a few minutes and a few GB of RAM.

Every solid in the step files is tessellated, then rays are cast at the whole
assembly from all round it. Solids no ray lands on (the electronics, screws,
clips and brackets) are dropped, and so are the faces of the kept solids that no
ray reaches (inner walls). What's left is one mesh per solid, named after its
file and index, in metres, which gltfpack simplifies and compresses.
site/js/headset.js picks materials by those names, so new step files can move
them: check with a colour per name before trusting the old list.
"""

import json
import subprocess
import sys
from pathlib import Path

import numpy as np
import trimesh
from OCP.BRep import BRep_Tool
from OCP.BRepMesh import BRepMesh_IncrementalMesh
from OCP.IFSelect import IFSelect_RetDone
from OCP.STEPControl import STEPControl_Reader
from OCP.TopAbs import TopAbs_FACE, TopAbs_REVERSED, TopAbs_SOLID
from OCP.TopExp import TopExp_Explorer
from OCP.TopLoc import TopLoc_Location
from OCP.TopoDS import TopoDS

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "site" / "model" / "steam-frame.glb"
DEFLECTION = 0.15      # mm the tessellation may stray from the true surface
DIRECTIONS = 160       # views round the assembly, and rays across each one
RAYS = 700
MIN_HITS = 100         # fewer rays than this on a solid and it's not really visible
DROP = {"strap9"}      # the strap file repeats the core's strap mount
SIMPLIFY = 0.12        # what gltfpack keeps of the triangles


def tessellate(step, prefix):
    """Every solid in a step file as (name, vertices in mm, triangles, face id per triangle)."""
    reader = STEPControl_Reader()
    if reader.ReadFile(str(step)) != IFSelect_RetDone:
        sys.exit(f"can't read {step}")
    reader.TransferRoots()
    shape = reader.OneShape()
    BRepMesh_IncrementalMesh(shape, DEFLECTION, False, 0.35, True)
    solids = []
    exp = TopExp_Explorer(shape, TopAbs_SOLID)
    while exp.More():
        verts, tris, faces = [], [], []
        fx = TopExp_Explorer(TopoDS.Solid(exp.Current()), TopAbs_FACE)
        face = 0
        while fx.More():
            shape_face = TopoDS.Face(fx.Current())
            loc = TopLoc_Location()
            tri = BRep_Tool.Triangulation_s(shape_face, loc)
            if tri is not None:
                base = len(verts)
                trsf = loc.Transformation()
                for k in range(1, tri.NbNodes() + 1):
                    p = tri.Node(k).Transformed(trsf)
                    verts.append((p.X(), p.Y(), p.Z()))
                flipped = shape_face.Orientation() == TopAbs_REVERSED
                for k in range(1, tri.NbTriangles() + 1):
                    a, b, c = tri.Triangle(k).Get()
                    if flipped:
                        b, c = c, b
                    tris.append((a - 1 + base, b - 1 + base, c - 1 + base))
                    faces.append(face)
            face += 1
            fx.Next()
        if tris:
            solids.append((f"{prefix}{len(solids)}", np.asarray(verts, np.float32), np.asarray(tris, np.int32), np.asarray(faces, np.int32)))
        exp.Next()
    print(f"{step.name}: {len(solids)} solids", flush=True)
    return solids


def visible(solids):
    """Rays from all round the assembly: how many land on each solid, and which triangles any reach."""
    offsets = np.cumsum([0] + [len(v) for _, v, _, _ in solids])
    mesh = trimesh.Trimesh(np.concatenate([v for _, v, _, _ in solids]), np.concatenate([t + o for (_, _, t, _), o in zip(solids, offsets)]), process=False)
    owner = np.concatenate([np.full(len(t), i) for i, (_, _, t, _) in enumerate(solids)])
    centre = mesh.bounds.mean(0)
    radius = np.linalg.norm(mesh.extents) / 2 + 5
    caster = trimesh.ray.ray_pyembree.RayMeshIntersector(mesh)
    grid = np.linspace(-radius, radius, RAYS)
    gu, gv = np.meshgrid(grid, grid)
    hits = np.zeros(len(solids), np.int64)
    reached = np.zeros(len(mesh.faces), bool)
    # a fibonacci sphere of view directions
    k = np.arange(DIRECTIONS) + 0.5
    phi, theta = np.arccos(1 - 2 * k / DIRECTIONS), np.pi * (1 + 5**0.5) * k
    for d in np.stack([np.cos(theta) * np.sin(phi), np.cos(phi), np.sin(theta) * np.sin(phi)], 1):
        up = np.array([0, 1, 0]) if abs(d[1]) < 0.9 else np.array([1, 0, 0])
        u = np.cross(up, d)
        u /= np.linalg.norm(u)
        v = np.cross(d, u)
        origins = centre - d * radius * 2 + gu.reshape(-1, 1) * u + gv.reshape(-1, 1) * v
        first = caster.intersects_first(origins, np.repeat(d[None], len(origins), 0))
        first = first[first >= 0]
        reached[first] = True
        np.add.at(hits, owner[first], 1)
    return hits, np.split(reached, np.cumsum([len(t) for _, _, t, _ in solids])[:-1])


def export(solids, hits, reached, out):
    """The visible solids, without their unseen faces, as one glb in metres."""
    scene = trimesh.Scene()
    kept = total = 0
    for (name, verts, tris, faces), seen, hit in zip(solids, hits, reached):
        total += len(tris)
        if seen < MIN_HITS or name in DROP:
            continue
        face_seen = np.zeros(faces.max() + 1, bool)
        face_seen[faces[hit]] = True
        tris = tris[face_seen[faces]]
        kept += len(tris)
        mesh = trimesh.Trimesh(verts / 1000.0, tris, process=False)
        mesh.remove_unreferenced_vertices()
        mesh.vertex_normals  # computed now, so cad faces keep their creases
        scene.add_geometry(mesh, node_name=name, geom_name=name)
    scene.export(out)
    print(f"{len(scene.geometry)} solids, {kept} of {total} triangles", flush=True)


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    out = Path(sys.argv[3]) if len(sys.argv) > 3 else OUT
    solids = tessellate(Path(sys.argv[1]), "core") + tessellate(Path(sys.argv[2]), "strap")
    hits, reached = visible(solids)
    full = out.with_suffix(".full.glb")
    export(solids, hits, reached, full)
    subprocess.run(["gltfpack", "-i", full, "-o", out, "-si", str(SIMPLIFY), "-kn", "-km", "-cc"], check=True)
    full.unlink()
    print(f"{out}: {out.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
