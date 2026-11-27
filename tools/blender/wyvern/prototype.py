"""ART-B2: Refine Primal Voxel wyvern voxel prototype (.blend + comparison renders).

Refinements over initial prototype:
- Folded posture wingspan ~3.5-3.7 m with wing/membrane ground clearance Z_min >= 0.20 m
  and clean flank tuck (zero hind-leg / torso clipping)
- Voxel-stepped faceted wing membranes + reinforced voxel finger spars
- Predatory tapered chest keel -> tucked waist -> hip line (replaces round belly)
- Sculpted wedge skull, integrated brow-horn collars, predatory S-curved neck,
  chipped right horn asymmetry, denser rhythmic dorsal/caudal spine crest
- Strictly <= 17,000 triangles, 4 material slots (Scales, Horns, Eyes, Membranes),
  no definitive skinning/rigging/animations, 1.80 m human reference at identical camera scale
"""

import argparse
import json
import math
import sys
import time
from pathlib import Path

import bmesh
import bpy
from mathutils import Euler, Vector


REPO_ROOT = Path(__file__).resolve().parents[3]
DEFAULT_BLEND = REPO_ROOT / "assets/creatures/wyvern_art/wyvern.blend"
DEFAULT_PREVIEW_DIR = REPO_ROOT / "art_pipeline/previews/wyvern"
DEFAULT_METRICS = REPO_ROOT / "art_pipeline/previews/wyvern/prototype_metrics.json"

PALETTE_JADE = {
    "Scales": (0.10, 0.38, 0.29, 1.0),
    "Horns": (0.66, 0.58, 0.39, 1.0),
    "Eyes": (1.00, 0.72, 0.20, 1.0),
    "Membranes": (0.18, 0.63, 0.55, 1.0),
}

# Discrete shade bands enable aggressive 2D greedy merging while keeping crisp voxel value contrast
SHADE_DORSAL = (0.72, 0.80, 0.76)
SHADE_MID = (0.94, 0.98, 0.94)
SHADE_FLANK = (0.86, 0.92, 0.88)
SHADE_BELLY = (1.28, 1.06, 0.72)
SHADE_DARK = (0.56, 0.64, 0.60)
SHADE_EYE_SOCKET = (0.18, 0.20, 0.18)
SHADE_HORN_BASE = (0.84, 0.80, 0.72)
SHADE_HORN_TIP = (1.04, 1.00, 0.92)


def srgb_to_linear(c):
    if c <= 0.04045:
        return c / 12.92
    return ((c + 0.055) / 1.055) ** 2.4


def make_material(name, rgba_srgb, roughness=0.74, emission=0.0, double_sided=False):
    mat = bpy.data.materials.get(name)
    if mat is None:
        mat = bpy.data.materials.new(name=name)
    mat.use_nodes = True
    mat.use_backface_culling = not double_sided
    nodes = mat.node_tree.nodes
    links = mat.node_tree.links
    nodes.clear()

    out = nodes.new("ShaderNodeOutputMaterial")
    out.location = (380, 0)

    bsdf = nodes.new("ShaderNodeBsdfPrincipled")
    bsdf.location = (80, 0)
    bsdf.inputs["Roughness"].default_value = roughness
    if "Specular IOR Level" in bsdf.inputs:
        bsdf.inputs["Specular IOR Level"].default_value = 0.26

    attr = nodes.new("ShaderNodeAttribute")
    attr.attribute_name = "Col"
    attr.location = (-360, 80)

    mix = nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    mix.blend_type = "MULTIPLY"
    mix.inputs["Factor"].default_value = 1.0
    mix.location = (-140, 80)

    lin = tuple(srgb_to_linear(v) for v in rgba_srgb[:3]) + (1.0,)
    mix.inputs["B"].default_value = lin

    links.new(attr.outputs["Color"], mix.inputs["A"])
    links.new(mix.outputs["Result"], bsdf.inputs["Base Color"])

    if emission > 0.0 and "Emission Color" in bsdf.inputs:
        bsdf.inputs["Emission Color"].default_value = lin
        bsdf.inputs["Emission Strength"].default_value = emission

    links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
    return mat


def make_simple_material(name, rgba_srgb, roughness=0.85):
    mat = bpy.data.materials.get(name)
    if mat is None:
        mat = bpy.data.materials.new(name=name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes.get("Principled BSDF")
    if bsdf:
        lin = tuple(srgb_to_linear(v) for v in rgba_srgb[:3]) + (1.0,)
        bsdf.inputs["Base Color"].default_value = lin
        bsdf.inputs["Roughness"].default_value = roughness
    return mat


class VoxelGrid:
    """Sparse 3D voxel grid with interior culling and 2D greedy face merging."""

    def __init__(self, step=0.06, min_z_world=None, origin=(0.0, 0.0, 0.0)):
        self.step = step
        self.min_z_world = min_z_world
        self.origin = origin
        self.cells = {}

    def _allowed_z(self, iz):
        if self.min_z_world is None:
            return True
        return (self.origin[2] + iz * self.step) >= (self.min_z_world - 1e-5)

    def add_world_ellipsoid(self, center, radii, mat_idx=0, shade_fn=None, origin=None):
        orig = self.origin if origin is None else origin
        cx, cy, cz = center[0] - orig[0], center[1] - orig[1], center[2] - orig[2]
        rx, ry, rz = radii
        s = self.step
        x0, x1 = math.floor((cx - rx) / s), math.ceil((cx + rx) / s)
        y0, y1 = math.floor((cy - ry) / s), math.ceil((cy + ry) / s)
        z0, z1 = math.floor((cz - rz) / s), math.ceil((cz + rz) / s)
        for ix in range(x0, x1 + 1):
            wx = (ix + 0.5) * s
            dx = (wx - cx) / max(rx, 1e-5)
            if abs(dx) > 1.0:
                continue
            for iy in range(y0, y1 + 1):
                wy = (iy + 0.5) * s
                dy = (wy - cy) / max(ry, 1e-5)
                if dx * dx + dy * dy > 1.0:
                    continue
                for iz in range(z0, z1 + 1):
                    if not self._allowed_z(iz):
                        continue
                    wz = (iz + 0.5) * s
                    dz = (wz - cz) / max(rz, 1e-5)
                    if dx * dx + dy * dy + dz * dz <= 1.0:
                        shade = shade_fn(wx + orig[0], wy + orig[1], wz + orig[2], dx, dy, dz) if shade_fn else SHADE_MID
                        self.cells[(ix, iy, iz)] = (mat_idx, shade)

    def add_world_capsule(
        self, p0, p1, r0, r1, mat_idx=0, shade_fn=None, origin=None,
        flatten_x=1.0, flatten_z=1.0, keel_power=0.0
    ):
        """Add an elliptical or V-keeled tapered capsule on the voxel grid."""
        orig = self.origin if origin is None else origin
        a = Vector(p0) - Vector(orig)
        b = Vector(p1) - Vector(orig)
        ab = b - a
        ab_len2 = ab.length_squared
        rmax = max(r0, r1)
        s = self.step
        min_v = Vector((min(a.x, b.x) - rmax * flatten_x, min(a.y, b.y) - rmax, min(a.z, b.z) - rmax * flatten_z))
        max_v = Vector((max(a.x, b.x) + rmax * flatten_x, max(a.y, b.y) + rmax, max(a.z, b.z) + rmax * flatten_z))
        x0, x1 = math.floor(min_v.x / s), math.ceil(max_v.x / s)
        y0, y1 = math.floor(min_v.y / s), math.ceil(max_v.y / s)
        z0, z1 = math.floor(min_v.z / s), math.ceil(max_v.z / s)

        for ix in range(x0, x1 + 1):
            wx = (ix + 0.5) * s
            for iy in range(y0, y1 + 1):
                wy = (iy + 0.5) * s
                for iz in range(z0, z1 + 1):
                    if not self._allowed_z(iz):
                        continue
                    wz = (iz + 0.5) * s
                    p = Vector((wx, wy, wz))
                    t = 0.0 if ab_len2 < 1e-9 else max(0.0, min(1.0, (p - a).dot(ab) / ab_len2))
                    closest = a + ab * t
                    rad = r0 * (1.0 - t) + r1 * t
                    delta = p - closest
                    # Optional V-keel narrows the lower belly/sternum for a predatory chest profile
                    z_norm = delta.z / max(rad * flatten_z, 1e-5)
                    x_scale = flatten_x
                    if keel_power > 0.0 and z_norm < 0.0:
                        x_scale = max(0.32, flatten_x * (1.0 + keel_power * z_norm))
                    nx = delta.x / max(rad * x_scale, 1e-5)
                    ny = delta.y / max(rad, 1e-5)
                    nz = z_norm
                    if nx * nx + ny * ny + nz * nz <= 1.0:
                        shade = shade_fn(wx + orig[0], wy + orig[1], wz + orig[2], t, nz) if shade_fn else SHADE_MID
                        self.cells[(ix, iy, iz)] = (mat_idx, shade)

    def add_world_box(self, bmin, bmax, mat_idx=0, shade=SHADE_MID, origin=None):
        orig = self.origin if origin is None else origin
        s = self.step
        x0 = math.floor((bmin[0] - orig[0]) / s + 1e-6)
        x1 = math.ceil((bmax[0] - orig[0]) / s - 1e-6) - 1
        y0 = math.floor((bmin[1] - orig[1]) / s + 1e-6)
        y1 = math.ceil((bmax[1] - orig[1]) / s - 1e-6) - 1
        z0 = math.floor((bmin[2] - orig[2]) / s + 1e-6)
        z1 = math.ceil((bmax[2] - orig[2]) / s - 1e-6) - 1
        for ix in range(x0, x1 + 1):
            for iy in range(y0, y1 + 1):
                for iz in range(z0, z1 + 1):
                    if not self._allowed_z(iz):
                        continue
                    self.cells[(ix, iy, iz)] = (mat_idx, shade)

    def build_mesh(self, name, materials):
        """Convert exposed voxel faces with 2D greedy merging into a crisp Blender Mesh."""
        bm = bmesh.new()
        color_layer = bm.loops.layers.color.new("Col")
        s = self.step

        dirs = [
            (0, +1, 1, 2, False),
            (0, -1, 1, 2, True),
            (1, +1, 0, 2, True),
            (1, -1, 0, 2, False),
            (2, +1, 0, 1, False),
            (2, -1, 0, 1, True),
        ]

        for axis, sign, u_ax, v_ax, flip in dirs:
            slices = {}
            for coord, val in self.cells.items():
                nbr = list(coord)
                nbr[axis] += sign
                if tuple(nbr) in self.cells:
                    continue
                k_slice = coord[axis]
                u, v = coord[u_ax], coord[v_ax]
                mat_idx, rgb = val
                qrgb = (round(rgb[0], 2), round(rgb[1], 2), round(rgb[2], 2))
                slices.setdefault(k_slice, {})[(u, v)] = (mat_idx, qrgb)

            for k_slice, grid in slices.items():
                visited = set()
                sorted_keys = sorted(grid.keys())
                plane_coord = (k_slice + (1 if sign > 0 else 0)) * s

                for (u0, v0) in sorted_keys:
                    if (u0, v0) in visited:
                        continue
                    token = grid[(u0, v0)]
                    w = 1
                    while (u0 + w, v0) in grid and (u0 + w, v0) not in visited and grid[(u0 + w, v0)] == token:
                        w += 1
                    h = 1
                    while True:
                        ok = True
                        for du in range(w):
                            key = (u0 + du, v0 + h)
                            if key not in grid or key in visited or grid[key] != token:
                                ok = False
                                break
                        if not ok:
                            break
                        h += 1

                    for du in range(w):
                        for dv in range(h):
                            visited.add((u0 + du, v0 + dv))

                    corners_uv = [
                        (u0 * s, v0 * s),
                        ((u0 + w) * s, v0 * s),
                        ((u0 + w) * s, (v0 + h) * s),
                        (u0 * s, (v0 + h) * s),
                    ]
                    if flip:
                        corners_uv = [corners_uv[0], corners_uv[3], corners_uv[2], corners_uv[1]]

                    verts = []
                    for cu, cv in corners_uv:
                        p = [0.0, 0.0, 0.0]
                        p[axis] = plane_coord
                        p[u_ax] = cu
                        p[v_ax] = cv
                        verts.append(bm.verts.new(p))

                    try:
                        face = bm.faces.new(verts)
                    except ValueError:
                        continue
                    mat_idx, qrgb = token
                    face.material_index = mat_idx
                    face.smooth = False
                    rgba = (qrgb[0], qrgb[1], qrgb[2], 1.0)
                    for loop in face.loops:
                        loop[color_layer] = rgba

        bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)
        bmesh.ops.triangulate(bm, faces=bm.faces)
        mesh = bpy.data.meshes.new(name)
        bm.to_mesh(mesh)
        bm.free()
        for mat in materials:
            mesh.materials.append(mat)
        return mesh


def build_voxel_membrane_mesh(name, pivot_pt, anchors, mat_idx_mem, origin, scallops=None, voxel_step=0.055):
    """Build thin, crisp voxel-stepped double-sided wing membranes.

    Uses quantized radial bands and stair-stepped trailing-edge scallops so the
    membrane border, camber facets, and internal rib stripes read in the exact
    same modern voxel language as the body without exploding triangle count.
    """
    bm = bmesh.new()
    color_layer = bm.loops.layers.color.new("Col")
    o = Vector(origin)
    p0 = Vector(pivot_pt) - o

    def qstep(val, step):
        return round(val / step) * step

    for idx in range(len(anchors) - 1):
        a = Vector(anchors[idx]) - o
        b = Vector(anchors[idx + 1]) - o
        scallop = scallops[idx] if scallops else 0.16
        rows = 5
        cols = 6  # 6 crisp stepped columns per bay -> pixel/voxel trailing notches
        grid = {}
        for r in range(rows + 1):
            t_r = r / rows
            for c in range(cols + 1):
                t_c = c / cols
                # Step t_c subtly at trailing half to produce blocky voxel-notch scallops
                pt = p0.lerp(a.lerp(b, t_c), t_r)
                if r > 0 and scallop > 0:
                    # Stepped triangle wave / discrete notch scallop instead of smooth sine
                    notch = 1.0 - abs(2.0 * t_c - 1.0)
                    notch_q = math.floor(notch * 3.0 + 0.5) / 3.0
                    inward = notch_q * (t_r ** 1.9) * scallop
                    vec = p0 - a.lerp(b, t_c)
                    if vec.length > 1e-5:
                        pt += vec.normalized() * inward
                # Quantize intermediate & trailing vertices to voxel grid increments
                if 0 < r <= rows and 0 < c < cols:
                    pt.x = qstep(pt.x, voxel_step)
                    pt.y = qstep(pt.y, voxel_step)
                    pt.z = qstep(pt.z, voxel_step * 0.8)
                # Subtle faceted camber step
                if 0 < r < rows:
                    camber = (0.045 if (r in (2, 3)) else 0.025) * (1.0 - abs(2.0 * t_c - 1.0) * 0.5)
                    pt.z += camber
                grid[(r, c)] = bm.verts.new(pt)

        for r in range(rows):
            for c in range(cols):
                v00 = grid[(r, c)]
                v10 = grid[(r + 1, c)]
                v11 = grid[(r + 1, c + 1)]
                v01 = grid[(r, c + 1)]
                # Crisp banded voxel rib-stripes: darker near spar edges, lighter mid-bay
                rib_shade = 0.74 if c in (0, cols - 1) else (0.95 if c in (2, 3) else 0.85)
                row_grad = 0.88 + 0.14 * (r / rows)
                # Trailing edge border slightly darker like a hemmed voxel pixel edge
                if r == rows - 1:
                    row_grad *= 0.82
                s_val = min(1.05, rib_shade * row_grad)
                col = (min(1.0, s_val * 0.96), min(1.0, s_val * 0.94), min(1.0, s_val * 0.90), 1.0)
                for tri_verts in ((v00, v10, v11), (v00, v11, v01)):
                    try:
                        f = bm.faces.new(tri_verts)
                        f.material_index = mat_idx_mem
                        f.smooth = False
                        for loop in f.loops:
                            loop[color_layer] = col
                    except ValueError:
                        pass

    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-4)
    mesh = bpy.data.meshes.new(name)
    bm.to_mesh(mesh)
    bm.free()
    return mesh


def link_obj(collection, obj):
    collection.objects.link(obj)
    return obj


def create_pivot(name, world_pos, parent, collection):
    empty = bpy.data.objects.new(name, None)
    empty.empty_display_type = "PLAIN_AXES"
    empty.empty_display_size = 0.16
    if parent is not None:
        empty.parent = parent
        empty.location = Vector(world_pos) - parent.matrix_world.translation
    else:
        empty.location = Vector(world_pos)
    link_obj(collection, empty)
    bpy.context.view_layer.update()
    return empty


def attach_grid_mesh(name, grid, materials, parent, collection):
    mesh = grid.build_mesh(name, materials)
    obj = bpy.data.objects.new(name, mesh)
    obj.parent = parent
    obj.location = (0.0, 0.0, 0.0)
    link_obj(collection, obj)
    return obj


def create_wyvern_prototype(scene, collection, mats, folded=False):
    """Create the refined articulated voxel wyvern prototype (ART-B2)."""
    # Joint coordinates in Blender space: +Z up, -Y forward (snout), +X left
    pts = {
        "Root": (0.0, 0.0, 0.0),
        "Body": (0.0, 0.13, 1.18),
        "NeckBase": (0.0, -0.58, 1.48),
        "NeckTip": (0.0, -1.08, 1.92),
        "Head": (0.0, -1.64, 2.24),
        "Jaw": (0.0, -1.66, 2.02),
        "TailBase": (0.0, 0.80, 1.19),
        "TailMid": (0.02, 1.70, 1.03),
        "TailTip": (0.06, 2.60, 0.80),
    }
    for side, sign in (("L", 1.0), ("R", -1.0)):
        pts[f"Hip.{side}"] = (0.40 * sign, 0.54, 1.15)
        pts[f"Knee.{side}"] = (0.52 * sign, 0.30, 0.65)
        pts[f"Ankle.{side}"] = (0.60 * sign, 0.78, 0.25)
        pts[f"WingShoulder.{side}"] = (0.43 * sign, -0.49, 1.50)
        pts[f"WingElbow.{side}"] = (1.19 * sign, -1.07, 1.65)
        pts[f"WingWrist.{side}"] = (2.03 * sign, -0.91, 1.79)
        pts[f"WingDigit.{side}"] = (2.77 * sign, -0.36, 1.56)

    joints = {}
    joints["Root"] = create_pivot("Root", pts["Root"], None, collection)
    joints["Body"] = create_pivot("Body", pts["Body"], joints["Root"], collection)
    joints["NeckBase"] = create_pivot("NeckBase", pts["NeckBase"], joints["Body"], collection)
    joints["NeckTip"] = create_pivot("NeckTip", pts["NeckTip"], joints["NeckBase"], collection)
    joints["Head"] = create_pivot("Head", pts["Head"], joints["NeckTip"], collection)
    joints["Jaw"] = create_pivot("Jaw", pts["Jaw"], joints["Head"], collection)
    joints["TailBase"] = create_pivot("TailBase", pts["TailBase"], joints["Body"], collection)
    joints["TailMid"] = create_pivot("TailMid", pts["TailMid"], joints["TailBase"], collection)
    joints["TailTip"] = create_pivot("TailTip", pts["TailTip"], joints["TailMid"], collection)

    for side in ("L", "R"):
        joints[f"Hip.{side}"] = create_pivot(f"Hip.{side}", pts[f"Hip.{side}"], joints["Body"], collection)
        joints[f"Knee.{side}"] = create_pivot(f"Knee.{side}", pts[f"Knee.{side}"], joints[f"Hip.{side}"], collection)
        joints[f"Ankle.{side}"] = create_pivot(f"Ankle.{side}", pts[f"Ankle.{side}"], joints[f"Knee.{side}"], collection)
        joints[f"WingShoulder.{side}"] = create_pivot(f"WingShoulder.{side}", pts[f"WingShoulder.{side}"], joints["Body"], collection)
        joints[f"WingElbow.{side}"] = create_pivot(f"WingElbow.{side}", pts[f"WingElbow.{side}"], joints[f"WingShoulder.{side}"], collection)
        joints[f"WingWrist.{side}"] = create_pivot(f"WingWrist.{side}", pts[f"WingWrist.{side}"], joints[f"WingElbow.{side}"], collection)
        joints[f"WingDigit.{side}"] = create_pivot(f"WingDigit.{side}", pts[f"WingDigit.{side}"], joints[f"WingWrist.{side}"], collection)

    SCALES, HORNS, EYES, MEMBRANES = 0, 1, 2, 3

    def belly_shade(wx, wy, wz, t, nz):
        if nz < -0.32:
            return SHADE_BELLY
        if nz > 0.56:
            return SHADE_DORSAL
        return SHADE_MID

    # 1. BODY — Taut, predatory chest keel tapering cleanly to a tucked waist & pelvis
    g_body = VoxelGrid(step=0.068, origin=pts["Body"])
    # Lean tucked waist -> pelvis
    g_body.add_world_capsule(
        (0, 0.84, 1.20), (0, 0.30, 1.26), 0.30, 0.38, SCALES, belly_shade,
        flatten_x=0.90, flatten_z=0.86, keel_power=0.38
    )
    # Tucked ribcage transition -> deep predatory thorax
    g_body.add_world_capsule(
        (0, 0.30, 1.26), (0, -0.34, 1.38), 0.38, 0.50, SCALES, belly_shade,
        flatten_x=0.92, flatten_z=0.96, keel_power=0.52
    )
    # Muscular shoulder girdle & chest front flowing toward neck base
    g_body.add_world_capsule(
        (0, -0.34, 1.38), (0, -0.68, 1.48), 0.48, 0.31, SCALES, belly_shade,
        flatten_x=0.92, flatten_z=0.92, keel_power=0.45
    )
    # Sharp thoracic sternum keel (narrow V-ridge instead of round belly)
    g_body.add_world_capsule(
        (0, -0.54, 1.26), (0, 0.04, 1.06), 0.16, 0.24, SCALES, lambda *a: SHADE_BELLY,
        flatten_x=0.58, flatten_z=1.10
    )
    # Shoulder blade ridges for muscular silhouette
    for sign in (1.0, -1.0):
        g_body.add_world_capsule(
            (0.28 * sign, -0.40, 1.56), (0.36 * sign, -0.05, 1.62), 0.14, 0.11, SCALES,
            lambda *a: SHADE_DORSAL
        )
    # Denser rhythmic dorsal spine crest along back (6 swept plates clearly visible in profile)
    for y_sp, z_sp, h_sp in (
        (-0.56, 1.78, 0.32),
        (-0.36, 1.84, 0.40),
        (-0.14, 1.86, 0.44),
        (0.10, 1.82, 0.40),
        (0.34, 1.76, 0.35),
        (0.58, 1.66, 0.29),
    ):
        g_body.add_world_capsule(
            (0, y_sp, z_sp), (0, y_sp + 0.14, z_sp + h_sp), 0.072, 0.022,
            HORNS, lambda *a: SHADE_HORN_BASE, flatten_x=0.50
        )
    attach_grid_mesh("Mesh_Body", g_body, mats, joints["Body"], collection)

    # 2. NECK — Predatory S-curve + ventral throat keel + continuous dorsal crest
    g_nb = VoxelGrid(step=0.058, origin=pts["NeckBase"])
    g_nb.add_world_capsule(
        (0, -0.46, 1.42), (0, -1.06, 1.88), 0.31, 0.23, SCALES, belly_shade,
        flatten_x=0.84, flatten_z=0.98, keel_power=0.35
    )
    for y_sp, z_sp, h_sp in ((-0.68, 1.66, 0.20), (-0.88, 1.84, 0.22)):
        g_nb.add_world_capsule(
            (0, y_sp, z_sp), (0, y_sp + 0.13, z_sp + h_sp), 0.068, 0.022,
            HORNS, lambda *a: SHADE_HORN_BASE, flatten_x=0.50
        )
    attach_grid_mesh("Mesh_NeckBase", g_nb, mats, joints["NeckBase"], collection)

    g_nt = VoxelGrid(step=0.054, origin=pts["NeckTip"])
    g_nt.add_world_capsule(
        (0, -1.02, 1.86), (0, -1.62, 2.22), 0.23, 0.20, SCALES, belly_shade,
        flatten_x=0.82, flatten_z=0.96, keel_power=0.30
    )
    for y_sp, z_sp, h_sp in ((-1.10, 2.02, 0.21), (-1.34, 2.20, 0.19)):
        g_nt.add_world_capsule(
            (0, y_sp, z_sp), (0, y_sp + 0.12, z_sp + h_sp), 0.062, 0.020,
            HORNS, lambda *a: SHADE_HORN_BASE, flatten_x=0.50
        )
    attach_grid_mesh("Mesh_NeckTip", g_nt, mats, joints["NeckTip"], collection)

    # 3. HEAD — Sharper wedge dragon skull, sculpted snout, integrated brow-horn collars, asymmetric horns
    g_head = VoxelGrid(step=0.048, origin=pts["Head"])
    # Back skull / temporal shield
    g_head.add_world_capsule(
        (0, -1.52, 2.24), (0, -1.88, 2.28), 0.28, 0.25, SCALES,
        lambda wx, wy, wz, t, nz: SHADE_DORSAL if nz > 0.25 else SHADE_MID,
        flatten_x=1.06, flatten_z=0.82, keel_power=0.2
    )
    # Tapered predatory muzzle
    g_head.add_world_capsule(
        (0, -1.88, 2.26), (0, -2.56, 2.20), 0.22, 0.095, SCALES,
        lambda wx, wy, wz, t, nz: SHADE_DORSAL if nz > 0.2 else SHADE_FLANK,
        flatten_x=0.95, flatten_z=0.72
    )
    # Hooked premaxilla snout tip to Y = -2.68 m
    g_head.add_world_box((-0.075, -2.68, 2.13), (0.075, -2.54, 2.25), SCALES, SHADE_DARK)
    # Nasal crest ridge
    g_head.add_world_capsule((0, -2.02, 2.36), (0, -2.52, 2.28), 0.065, 0.035, SCALES, lambda *a: SHADE_DORSAL, flatten_x=0.6)

    for sign in (1.0, -1.0):
        # Sweeping brow ridge flowing into horn collar
        g_head.add_world_capsule(
            (0.24 * sign, -1.66, 2.40), (0.21 * sign, -2.06, 2.35), 0.082, 0.050,
            SCALES, lambda *a: SHADE_DARK
        )
        # Integrated armored horn socket/collar on cranium
        g_head.add_world_capsule(
            (0.20 * sign, -1.68, 2.38), (0.29 * sign, -1.52, 2.50), 0.135, 0.115,
            SCALES, lambda *a: SHADE_DORSAL
        )
        # Sculpted zygomatic cheek flare
        g_head.add_world_capsule(
            (0.27 * sign, -1.62, 2.17), (0.18 * sign, -2.14, 2.15), 0.085, 0.045,
            SCALES, lambda *a: SHADE_FLANK
        )
        # Dark recessed eye socket + glowing amber eye
        g_head.add_world_box(
            (0.25 * sign - 0.04, -1.96, 2.26), (0.25 * sign + 0.04, -1.80, 2.36),
            SCALES, SHADE_EYE_SOCKET
        )
        g_head.add_world_box(
            (0.27 * sign - 0.04, -1.92, 2.28), (0.27 * sign + 0.04, -1.84, 2.34),
            EYES, (1.0, 1.0, 1.0)
        )
        # Main swept horns (Left intact Z=2.82 m; Right chipped shorter Z=2.69 m)
        horn_tip = (0.49 * sign, -1.04 if sign > 0 else -1.22, 2.815 if sign > 0 else 2.69)
        g_head.add_world_capsule(
            (0.25 * sign, -1.58, 2.46), (0.38 * sign, -1.34, 2.64), 0.11, 0.068,
            HORNS, lambda *a: SHADE_HORN_BASE
        )
        g_head.add_world_capsule(
            (0.38 * sign, -1.34, 2.64), horn_tip, 0.068, 0.024 if sign > 0 else 0.042,
            HORNS, lambda *a: SHADE_HORN_TIP
        )
        # Lower cheek horn swept back from cheekbone
        g_head.add_world_capsule(
            (0.24 * sign, -1.62, 2.22), (0.42 * sign, -1.24, 2.29), 0.068, 0.022,
            HORNS, lambda *a: SHADE_HORN_BASE
        )
        # Upper fangs
        g_head.add_world_box((0.11 * sign - 0.025, -2.44, 2.08), (0.11 * sign + 0.025, -2.37, 2.16), HORNS, SHADE_HORN_TIP)
        g_head.add_world_box((0.15 * sign - 0.025, -2.24, 2.10), (0.15 * sign + 0.025, -2.17, 2.16), HORNS, SHADE_HORN_TIP)
    # Right-cheek scar accent (subtle weathered dark-green streak, distinct from eye socket)
    g_head.add_world_box((-0.24, -2.20, 2.15), (-0.18, -2.04, 2.19), SCALES, SHADE_DARK)
    attach_grid_mesh("Mesh_Head", g_head, mats, joints["Head"], collection)

    # 4. ARTICULATED JAW — Slimmer profile + chin spur
    g_jaw = VoxelGrid(step=0.048, origin=pts["Jaw"])
    g_jaw.add_world_capsule(
        (0, -1.64, 2.04), (0, -2.52, 2.07), 0.22, 0.088, SCALES,
        lambda *a: SHADE_BELLY, flatten_x=0.98, flatten_z=0.55
    )
    g_jaw.add_world_capsule((0, -1.78, 1.98), (0, -1.62, 1.88), 0.05, 0.022, HORNS, lambda *a: SHADE_HORN_BASE)
    for sign in (1.0, -1.0):
        g_jaw.add_world_box((0.10 * sign - 0.025, -2.42, 2.09), (0.10 * sign + 0.025, -2.35, 2.15), HORNS, SHADE_HORN_TIP)
    attach_grid_mesh("Mesh_Jaw", g_jaw, mats, joints["Jaw"], collection)
    joints["Jaw"].rotation_euler = Euler((0.08, 0.0, 0.0), "XYZ")

    # 5. TAIL — Denser caudal spine crest + asymmetric spade tail fin to Y = +3.70 m
    g_tb = VoxelGrid(step=0.064, origin=pts["TailBase"])
    g_tb.add_world_capsule((0, 0.66, 1.21), (0, 1.70, 1.03), 0.29, 0.19, SCALES, belly_shade, keel_power=0.25)
    for y_sp, z_sp, h_sp in ((0.90, 1.44, 0.18), (1.18, 1.38, 0.17), (1.46, 1.30, 0.16)):
        g_tb.add_world_capsule((0, y_sp, z_sp), (0, y_sp + 0.12, z_sp + h_sp), 0.055, 0.02, HORNS, lambda *a: SHADE_HORN_BASE, flatten_x=0.5)
    attach_grid_mesh("Mesh_TailBase", g_tb, mats, joints["TailBase"], collection)

    g_tm = VoxelGrid(step=0.056, origin=pts["TailMid"])
    g_tm.add_world_capsule((0.0, 1.66, 1.04), (0.06, 2.60, 0.80), 0.19, 0.11, SCALES, belly_shade)
    for y_sp, z_sp, x_sp, h_sp in ((1.76, 1.20, 0.01, 0.15), (2.06, 1.12, 0.03, 0.14), (2.36, 1.02, 0.05, 0.13)):
        g_tm.add_world_capsule((x_sp, y_sp, z_sp), (x_sp, y_sp + 0.11, z_sp + h_sp), 0.048, 0.018, HORNS, lambda *a: SHADE_HORN_BASE, flatten_x=0.5)
    attach_grid_mesh("Mesh_TailMid", g_tm, mats, joints["TailMid"], collection)

    g_tt = VoxelGrid(step=0.050, origin=pts["TailTip"])
    g_tt.add_world_capsule((0.06, 2.56, 0.81), (0.01, 3.12, 0.84), 0.11, 0.065, SCALES, belly_shade)
    g_tt.add_world_capsule((0.01, 3.12, 0.84), (-0.16, 3.63, 1.02), 0.065, 0.032, SCALES, belly_shade)
    for y_sp, z_sp, x_sp in ((2.74, 0.94, 0.04), (3.02, 0.94, 0.01)):
        g_tt.add_world_capsule((x_sp, y_sp, z_sp), (x_sp, y_sp + 0.09, z_sp + 0.11), 0.04, 0.016, HORNS, lambda *a: SHADE_HORN_BASE, flatten_x=0.5)
    g_tt.add_world_ellipsoid((-0.18, 3.53, 1.02), (0.14, 0.16, 0.045), HORNS, lambda *a: SHADE_HORN_BASE)
    attach_grid_mesh("Mesh_TailTip", g_tt, mats, joints["TailTip"], collection)

    # 6. HIND LEGS — Digitigrade stance, grounded cleanly at Z = 0.00 m
    for side, sign in (("L", 1.0), ("R", -1.0)):
        h_pt = pts[f"Hip.{side}"]
        k_pt = pts[f"Knee.{side}"]
        a_pt = pts[f"Ankle.{side}"]

        g_hip = VoxelGrid(step=0.058, origin=h_pt)
        g_hip.add_world_capsule((0.26 * sign, 0.58, 1.24), h_pt, 0.23, 0.27, SCALES, belly_shade)
        g_hip.add_world_capsule(h_pt, k_pt, 0.26, 0.16, SCALES, belly_shade)
        attach_grid_mesh(f"Mesh_Hip.{side}", g_hip, mats, joints[f"Hip.{side}"], collection)

        g_knee = VoxelGrid(step=0.050, origin=k_pt)
        g_knee.add_world_capsule(k_pt, a_pt, 0.155, 0.105, SCALES, belly_shade)
        attach_grid_mesh(f"Mesh_Knee.{side}", g_knee, mats, joints[f"Knee.{side}"], collection)

        g_ankle = VoxelGrid(step=0.050, min_z_world=0.0, origin=a_pt)
        foot_base = (0.60 * sign, 0.54, 0.09)
        g_ankle.add_world_capsule(a_pt, foot_base, 0.10, 0.105, SCALES, belly_shade)
        g_ankle.add_world_box((0.60 * sign - 0.14, 0.44, 0.00), (0.60 * sign + 0.14, 0.70, 0.12), SCALES, SHADE_BELLY)
        for ti in (-1, 0, 1):
            tx = 0.60 * sign + ti * 0.11
            ty_end = 0.24 - (0.06 if ti == 0 else 0.0)
            g_ankle.add_world_capsule((0.60 * sign + ti * 0.06, 0.48, 0.06), (tx, ty_end, 0.05), 0.055, 0.04, SCALES, lambda *a: SHADE_MID)
            g_ankle.add_world_capsule((tx, ty_end, 0.05), (tx, ty_end - 0.11, 0.02), 0.04, 0.02, HORNS, lambda *a: SHADE_HORN_BASE)
        g_ankle.add_world_capsule((0.60 * sign, 0.72, 0.10), (0.60 * sign, 0.88, 0.06), 0.045, 0.02, HORNS, lambda *a: SHADE_HORN_BASE)
        attach_grid_mesh(f"Mesh_Ankle.{side}", g_ankle, mats, joints[f"Ankle.{side}"], collection)

    # 7. WINGS — Reinforced finger spars + voxel-stepped membranes + clean folded pose
    for side, sign in (("L", 1.0), ("R", -1.0)):
        ws = pts[f"WingShoulder.{side}"]
        we = pts[f"WingElbow.{side}"]
        ww = pts[f"WingWrist.{side}"]
        wd = pts[f"WingDigit.{side}"]

        g_ws = VoxelGrid(step=0.056, origin=ws)
        g_ws.add_world_capsule((0.26 * sign, -0.40, 1.45), ws, 0.21, 0.19, SCALES, belly_shade)
        g_ws.add_world_capsule(ws, we, 0.18, 0.125, SCALES, belly_shade)
        attach_grid_mesh(f"Mesh_WingShoulder.{side}", g_ws, mats, joints[f"WingShoulder.{side}"], collection)

        g_we = VoxelGrid(step=0.050, origin=we)
        g_we.add_world_capsule(we, ww, 0.125, 0.095, SCALES, belly_shade)
        attach_grid_mesh(f"Mesh_WingElbow.{side}", g_we, mats, joints[f"WingElbow.{side}"], collection)

        g_ww = VoxelGrid(step=0.046, origin=ww)
        g_ww.add_world_capsule(ww, wd, 0.095, 0.055, SCALES, belly_shade)
        # Reinforced armored wrist knuckle & hooked thumb claw
        g_ww.add_world_ellipsoid(ww, (0.11, 0.11, 0.10), SCALES, lambda *a: SHADE_DORSAL)
        thumb_mid = (2.06 * sign, -1.10, 1.89)
        thumb_tip = (1.96 * sign, -1.24, 1.81)
        g_ww.add_world_capsule(ww, thumb_mid, 0.068, 0.042, HORNS, lambda *a: SHADE_HORN_BASE)
        g_ww.add_world_capsule(thumb_mid, thumb_tip, 0.042, 0.02, HORNS, lambda *a: SHADE_HORN_TIP)

        # Reinforced secondary wing finger spars
        f2_tip = (3.15 * sign, 1.25, 1.19)
        f3_tip = (2.48 * sign, 1.68, 1.11)
        f4_tip = (1.60 * sign, 1.64, 1.08)
        for ft in (f2_tip, f3_tip, f4_tip):
            g_ww.add_world_capsule(ww, ft, 0.052, 0.026, SCALES, lambda *a: SHADE_DORSAL)
        attach_grid_mesh(f"Mesh_WingWrist.{side}", g_ww, mats, joints[f"WingWrist.{side}"], collection)

        # Outer wingtip spar hitting ±3.654 m (7.308 m open wingspan)
        tip_outer = (3.55 * sign, 0.36, 1.25)
        g_wd = VoxelGrid(step=0.042, origin=wd)
        g_wd.add_world_capsule(wd, tip_outer, 0.058, 0.028, SCALES, lambda *a: SHADE_DORSAL)
        g_wd.add_world_box(
            (min(3.55 * sign, 3.63 * sign), 0.33, 1.22),
            (max(3.55 * sign, 3.63 * sign), 0.40, 1.28),
            HORNS, SHADE_HORN_BASE
        )
        attach_grid_mesh(f"Mesh_WingDigit.{side}", g_wd, mats, joints[f"WingDigit.{side}"], collection)

        if not folded:
            mem_outer = build_voxel_membrane_mesh(
                f"Mesh_MembraneOuter.{side}",
                ww,
                [wd, tip_outer, f2_tip, f3_tip, f4_tip],
                MEMBRANES,
                ww,
                scallops=[0.05, 0.24, 0.26, 0.24],
                voxel_step=0.055,
            )
            for m in mats:
                mem_outer.materials.append(m)
            obj_mo = bpy.data.objects.new(f"Mesh_MembraneOuter.{side}", mem_outer)
            obj_mo.parent = joints[f"WingWrist.{side}"]
            link_obj(collection, obj_mo)

            mem_inner = build_voxel_membrane_mesh(
                f"Mesh_MembraneInner.{side}",
                (0.42 * sign, 0.64, 1.15),
                [ws, we, ww, f4_tip],
                MEMBRANES,
                pts["Body"],
                scallops=[0.0, 0.0, 0.0],
                voxel_step=0.055,
            )
            for m in mats:
                mem_inner.materials.append(m)
            obj_mi = bpy.data.objects.new(f"Mesh_MembraneInner.{side}", mem_inner)
            obj_mi.parent = joints["Body"]
            link_obj(collection, obj_mi)
        else:
            # Ground idle folded-wings posture:
            # Aim Z-fold chain deterministically for 3.5-3.7 m wingspan, Z_min >= 0.20 m, zero leg/torso clip
            joints["Body"].rotation_euler = Euler((0.04, 0.0, 0.0), "XYZ")
            joints["NeckBase"].rotation_euler = Euler((-0.05, 0.0, 0.0), "XYZ")
            joints["NeckTip"].rotation_euler = Euler((0.05, 0.0, 0.0), "XYZ")
            bpy.context.view_layer.update()

            def aim_joint(joint_obj, rest_dir_world, desired_dir_world, axial_roll=0.0):
                p_rot = joint_obj.parent.matrix_world.to_3x3()
                local_rest = rest_dir_world.normalized()
                local_desired = (p_rot.inverted() @ desired_dir_world).normalized()
                q = local_rest.rotation_difference(local_desired)
                if abs(axial_roll) > 1e-5:
                    from mathutils import Quaternion
                    q = Quaternion(local_desired, axial_roll) @ q
                joint_obj.rotation_euler = q.to_euler("XYZ")
                bpy.context.view_layer.update()

            # 1. Shoulder -> Elbow tucks back & outward cleanly clear of flank/hip
            elbow_target_dir = Vector((1.08 * sign, 0.26, 1.64)) - joints[f"WingShoulder.{side}"].matrix_world.translation
            aim_joint(joints[f"WingShoulder.{side}"], Vector(we) - Vector(ws), elbow_target_dir)

            # 2. Elbow -> Wrist folds forward & elevated clearly outside shoulder/chest
            wrist_target_dir = Vector((1.36 * sign, -0.42, 1.98)) - joints[f"WingElbow.{side}"].matrix_world.translation
            aim_joint(joints[f"WingElbow.{side}"], Vector(ww) - Vector(we), wrist_target_dir, axial_roll=-0.12 * sign)

            # 3. Wrist -> Digit sweeps fingers backward & elevated (~3.53-3.65 m wide, Z_min >= 0.45 m)
            digit_target_dir = Vector((1.75 * sign, 1.18, 1.90)) - joints[f"WingWrist.{side}"].matrix_world.translation
            aim_joint(joints[f"WingWrist.{side}"], Vector(wd) - Vector(ww), digit_target_dir, axial_roll=-0.24 * sign)
            joints[f"WingDigit.{side}"].rotation_euler = Euler((0.02, 0.0, 0.06 * sign), "XYZ")
            bpy.context.view_layer.update()

            flank_w = joints["Body"].matrix_world @ (Vector((0.48 * sign, 0.58, 1.24)) - Vector(pts["Body"]))
            ws_w = joints[f"WingShoulder.{side}"].matrix_world.translation
            we_w = joints[f"WingElbow.{side}"].matrix_world.translation
            ww_w = joints[f"WingWrist.{side}"].matrix_world.translation
            wd_w = joints[f"WingDigit.{side}"].matrix_world.translation

            tip_w = joints[f"WingDigit.{side}"].matrix_world @ (Vector(tip_outer) - Vector(wd))
            f2_w = joints[f"WingWrist.{side}"].matrix_world @ (Vector(f2_tip) - Vector(ww))
            f3_w = joints[f"WingWrist.{side}"].matrix_world @ (Vector(f3_tip) - Vector(ww))
            f4_w = joints[f"WingWrist.{side}"].matrix_world @ (Vector(f4_tip) - Vector(ww))

            mem_folded = build_voxel_membrane_mesh(
                f"Mesh_MembraneFolded.{side}",
                ww_w,
                [wd_w, tip_w, f2_w, f3_w, f4_w, flank_w],
                MEMBRANES,
                (0.0, 0.0, 0.0),
                scallops=[0.03, 0.12, 0.12, 0.10, 0.14],
                voxel_step=0.050,
            )
            for m in mats:
                mem_folded.materials.append(m)
            obj_mf = bpy.data.objects.new(f"Mesh_MembraneFolded.{side}", mem_folded)
            obj_mf.parent = joints["Root"]
            link_obj(collection, obj_mf)

            mem_arm = build_voxel_membrane_mesh(
                f"Mesh_MembraneFoldedArm.{side}",
                flank_w,
                [we_w, ww_w],
                MEMBRANES,
                (0.0, 0.0, 0.0),
                scallops=[0.0],
                voxel_step=0.050,
            )
            for m in mats:
                mem_arm.materials.append(m)
            obj_ma = bpy.data.objects.new(f"Mesh_MembraneFoldedArm.{side}", mem_arm)
            obj_ma.parent = joints["Root"]
            link_obj(collection, obj_ma)

    bpy.context.view_layer.update()
    return joints


def create_human_reference(collection, location=(2.20, -2.35, 0.0)):
    """Create a stylized 1.80 m voxel human silhouette grounded at Z=0.00."""
    mat_human = make_simple_material("HumanScaleRef", (0.58, 0.64, 0.72, 1.0), roughness=0.78)
    g = VoxelGrid(step=0.05)
    for sx in (-0.11, 0.11):
        g.add_world_box((sx - 0.075, -0.075, 0.00), (sx + 0.075, 0.075, 0.85), 0, (0.85, 0.88, 0.92))
        g.add_world_box((sx - 0.075, -0.15, 0.00), (sx + 0.075, 0.05, 0.10), 0, (0.72, 0.75, 0.80))
    g.add_world_box((-0.20, -0.11, 0.85), (0.20, 0.11, 1.45), 0, (0.95, 0.98, 1.0))
    for sx in (-0.28, 0.28):
        g.add_world_box((sx - 0.06, -0.06, 0.80), (sx + 0.06, 0.06, 1.45), 0, (0.88, 0.92, 0.95))
    g.add_world_box((-0.06, -0.06, 1.45), (0.06, 0.06, 1.52), 0, (0.9, 0.9, 0.9))
    g.add_world_box((-0.11, -0.11, 1.52), (0.11, 0.11, 1.80), 0, (1.0, 1.0, 1.0))

    mesh = g.build_mesh("Human_180cm_Mesh", [mat_human])
    obj = bpy.data.objects.new("Human_ScaleRef_180cm", mesh)
    obj.location = Vector(location)
    link_obj(collection, obj)
    return obj


def measure_collection_bounds_and_tris(collection):
    depsgraph = bpy.context.evaluated_depsgraph_get()
    min_pt = Vector((float("inf"), float("inf"), float("inf")))
    max_pt = Vector((float("-inf"), float("-inf"), float("-inf")))
    wing_min_z = float("inf")
    total_tris = 0
    total_verts = 0
    mesh_count = 0

    for obj in collection.all_objects:
        if obj.type != "MESH":
            continue
        mesh_count += 1
        eval_obj = obj.evaluated_get(depsgraph)
        me = eval_obj.to_mesh()
        me.calc_loop_triangles()
        total_tris += len(me.loop_triangles)
        total_verts += len(me.vertices)
        mat_w = obj.matrix_world
        is_wing = ("Wing" in obj.name) or ("Membrane" in obj.name)
        for v in me.vertices:
            wp = mat_w @ v.co
            min_pt.x = min(min_pt.x, wp.x)
            min_pt.y = min(min_pt.y, wp.y)
            min_pt.z = min(min_pt.z, wp.z)
            max_pt.x = max(max_pt.x, wp.x)
            max_pt.y = max(max_pt.y, wp.y)
            max_pt.z = max(max_pt.z, wp.z)
            if is_wing:
                wing_min_z = min(wing_min_z, wp.z)
        eval_obj.to_mesh_clear()

    dims = max_pt - min_pt
    return {
        "mesh_objects": mesh_count,
        "vertices": total_verts,
        "triangles": total_tris,
        "bounds_min_m": [round(min_pt.x, 4), round(min_pt.y, 4), round(min_pt.z, 4)],
        "bounds_max_m": [round(max_pt.x, 4), round(max_pt.y, 4), round(max_pt.z, 4)],
        "width_x_m": round(dims.x, 4),
        "length_y_m": round(dims.y, 4),
        "height_z_m": round(dims.z, 4),
        "wing_and_membrane_min_z_m": round(wing_min_z, 4) if wing_min_z < float("inf") else None,
    }


def setup_render_stage(scene, add_ground=True):
    scene.unit_settings.system = "METRIC"
    scene.unit_settings.scale_length = 1.0
    scene.render.engine = "BLENDER_EEVEE_NEXT"
    scene.render.resolution_x = 1280
    scene.render.resolution_y = 800
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.view_settings.view_transform = "Standard"
    scene.view_settings.look = "None"
    scene.view_settings.exposure = 0.0

    world = bpy.data.worlds.new(f"StudioWorld_{scene.name}")
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes.get("Background")
    if bg:
        bg.inputs["Color"].default_value = (0.16, 0.19, 0.23, 1.0)
        bg.inputs["Strength"].default_value = 0.9

    if not add_ground:
        return None

    stage_col = bpy.data.collections.new(f"Stage_{scene.name}")
    scene.collection.children.link(stage_col)

    bm = bmesh.new()
    bmesh.ops.create_grid(bm, x_segments=1, y_segments=1, size=9.0)
    g_mesh = bpy.data.meshes.new(f"Ground_{scene.name}")
    bm.to_mesh(g_mesh)
    bm.free()
    g_mat = make_simple_material("GroundPlane", (0.20, 0.22, 0.25, 1.0), roughness=0.92)
    g_mesh.materials.append(g_mat)
    g_obj = bpy.data.objects.new(f"Ground_{scene.name}", g_mesh)
    g_obj.location = (0.0, 0.3, -0.002)
    stage_col.objects.link(g_obj)

    for lname, ltype, loc, rot_deg, energy, size in (
        ("KeyLight", "AREA", (5.5, -5.5, 6.5), (48, 0, 45), 1400.0, 4.5),
        ("FillLight", "AREA", (-6.0, -3.5, 3.8), (62, 0, -58), 550.0, 5.5),
        ("RimLight", "AREA", (-2.0, 6.5, 5.0), (-50, 0, 165), 850.0, 4.0),
    ):
        ldata = bpy.data.lights.new(name=f"{lname}_{scene.name}", type=ltype)
        ldata.energy = energy
        if ltype == "AREA":
            ldata.size = size
        lobj = bpy.data.objects.new(name=f"{lname}_{scene.name}", object_data=ldata)
        lobj.location = loc
        lobj.rotation_euler = Euler(tuple(math.radians(a) for a in rot_deg), "XYZ")
        stage_col.objects.link(lobj)

    cam_data = bpy.data.cameras.new(name=f"Cam_{scene.name}")
    cam_data.clip_start = 0.1
    cam_data.clip_end = 100.0
    cam_obj = bpy.data.objects.new(name=f"Cam_{scene.name}", object_data=cam_data)
    stage_col.objects.link(cam_obj)
    scene.camera = cam_obj
    return cam_obj


def point_camera_at(cam_obj, eye, target, ortho=False, ortho_scale=8.5, lens=38.0):
    cam_obj.location = Vector(eye)
    direction = Vector(target) - Vector(eye)
    cam_obj.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
    if ortho:
        cam_obj.data.type = "ORTHO"
        cam_obj.data.ortho_scale = ortho_scale
    else:
        cam_obj.data.type = "PERSP"
        cam_obj.data.lens = lens


def render_view(scene, cam_obj, eye, target, out_path, ortho=False, ortho_scale=8.5, lens=38.0):
    bpy.context.window.scene = scene
    point_camera_at(cam_obj, eye, target, ortho=ortho, ortho_scale=ortho_scale, lens=lens)
    scene.render.filepath = str(out_path)
    t0 = time.perf_counter()
    bpy.ops.render.render(write_still=True)
    elapsed = time.perf_counter() - t0
    return round(elapsed, 3)


def clear_all_data():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--blend", type=Path, default=DEFAULT_BLEND)
    parser.add_argument("--previews", type=Path, default=DEFAULT_PREVIEW_DIR)
    parser.add_argument("--metrics", type=Path, default=DEFAULT_METRICS)
    parser.add_argument("--skip-render", action="store_true")
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else [])

    clear_all_data()

    mat_scales = make_material("Scales", PALETTE_JADE["Scales"], roughness=0.76)
    mat_horns = make_material("Horns", PALETTE_JADE["Horns"], roughness=0.62)
    mat_eyes = make_material("Eyes", PALETTE_JADE["Eyes"], roughness=0.28, emission=0.8)
    mat_mem = make_material("Membranes", PALETTE_JADE["Membranes"], roughness=0.84, double_sided=True)
    mats = [mat_scales, mat_horns, mat_eyes, mat_mem]

    # 1. Scene `Wyvern` (clean open-wings prototype)
    scene_open = bpy.context.scene
    scene_open.name = "Wyvern"
    scene_open.unit_settings.system = "METRIC"
    scene_open.unit_settings.scale_length = 1.0
    col_open = bpy.data.collections.new("Wyvern_Open")
    scene_open.collection.children.link(col_open)
    create_wyvern_prototype(scene_open, col_open, mats, folded=False)
    bpy.context.view_layer.update()
    metrics_open = measure_collection_bounds_and_tris(col_open)

    # 2. Scene `Wyvern_Folded` (folded-wings posture at origin)
    scene_folded = bpy.data.scenes.new("Wyvern_Folded")
    bpy.context.window.scene = scene_folded
    scene_folded.unit_settings.system = "METRIC"
    scene_folded.unit_settings.scale_length = 1.0
    col_folded = bpy.data.collections.new("Wyvern_Folded")
    scene_folded.collection.children.link(col_folded)
    create_wyvern_prototype(scene_folded, col_folded, mats, folded=True)
    bpy.context.view_layer.update()
    metrics_folded = measure_collection_bounds_and_tris(col_folded)

    # 3. Scene `Presentation` (lighting, camera, ground + 1.80 m human scale ref)
    scene_pres = bpy.data.scenes.new("Presentation")
    bpy.context.window.scene = scene_pres
    cam_pres = setup_render_stage(scene_pres, add_ground=True)
    scene_pres.collection.children.link(col_open)
    col_human = bpy.data.collections.new("ScaleReference_Human_180cm")
    scene_pres.collection.children.link(col_human)
    create_human_reference(col_human, location=(2.20, -2.35, 0.0))
    bpy.context.view_layer.update()
    metrics_human = measure_collection_bounds_and_tris(col_human)

    # 4. Scene `Presentation_Folded`
    scene_pres_folded = bpy.data.scenes.new("Presentation_Folded")
    bpy.context.window.scene = scene_pres_folded
    cam_folded = setup_render_stage(scene_pres_folded, add_ground=True)
    scene_pres_folded.collection.children.link(col_folded)
    scene_pres_folded.collection.children.link(col_human)
    bpy.context.view_layer.update()

    bpy.context.window.scene = scene_open
    args.blend.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(args.blend), compress=True)

    # Shared camera definitions so Open and Folded pairs have IDENTICAL visual scale
    cam_front = ((0.0, -9.5, 1.45), (0.15, 0.0, 1.35), True, 8.4, 36.0)
    cam_profile = ((-9.5, 0.45, 1.45), (0.0, 0.45, 1.35), True, 7.6, 36.0)
    cam_top = ((0.0, 0.50, 11.5), (0.0, 0.50, 1.2), True, 10.6, 36.0)
    cam_3q = ((6.2, -6.8, 3.6), (0.2, 0.25, 1.25), False, 8.4, 36.0)

    render_times = {}
    if not args.skip_render:
        args.previews.mkdir(parents=True, exist_ok=True)
        for fname, sc, cam_o, cfg in (
            ("open_front.png", scene_pres, cam_pres, cam_front),
            ("open_profile.png", scene_pres, cam_pres, cam_profile),
            ("open_top.png", scene_pres, cam_pres, cam_top),
            ("open_three_quarter.png", scene_pres, cam_pres, cam_3q),
            ("folded_front.png", scene_pres_folded, cam_folded, cam_front),
            ("folded_profile.png", scene_pres_folded, cam_folded, cam_profile),
            ("folded_three_quarter.png", scene_pres_folded, cam_folded, cam_3q),
        ):
            eye, tgt, ortho, o_scale, lens = cfg
            render_times[fname] = render_view(
                sc, cam_o, eye, tgt, args.previews / fname,
                ortho=ortho, ortho_scale=o_scale, lens=lens
            )

    payload = {
        "pass": "ART-B2",
        "blender_version": bpy.app.version_string,
        "blend_path": str(args.blend),
        "blend_size_bytes": args.blend.stat().st_size,
        "wyvern_open": metrics_open,
        "wyvern_folded": metrics_folded,
        "human_reference": metrics_human,
        "render_times_sec": render_times,
        "total_render_time_sec": round(sum(render_times.values()), 3),
    }
    args.metrics.parent.mkdir(parents=True, exist_ok=True)
    args.metrics.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    print(json.dumps(payload, indent=2))


if __name__ == "__main__":
    main()
