"""ART-C1: Unified topological voxel wyvern, 23-bone armature, normalized skinning,
deformation tests, and technical GLB verification export/re-import roundtrip.

Creates:
- Single unified skinned mesh `Wyvern_Mesh` (Scales, Horns, Eyes, Membranes)
- Hierarchical 23-bone Armature `Wyvern_Rig` matching docs/WYVERN_ASSET.md
- Normalized vertex weights (<= 4 influences, sum == 1.0, 0 unassigned)
- 7 technical pose checks on the exact same topology without mesh rebuild
- Technical GLB export + binary audit + Blender glTF roundtrip re-import test
- Diagnostic renders in art_pipeline/previews/wyvern/art_c1/
"""

import argparse
import json
import math
import struct
import sys
import time
from collections import defaultdict
from pathlib import Path

import bmesh
import bpy
from mathutils import Euler, Matrix, Quaternion, Vector


REPO_ROOT = Path(__file__).resolve().parents[3]
DEFAULT_BLEND = REPO_ROOT / "assets/creatures/wyvern_art/wyvern.blend"
DEFAULT_GLB = REPO_ROOT / "assets/creatures/wyvern_art/wyvern_tech.glb"
DEFAULT_MANIFEST = REPO_ROOT / "assets/creatures/wyvern_art.json"
DEFAULT_PREVIEW_DIR = REPO_ROOT / "art_pipeline/previews/wyvern/art_c1"
DEFAULT_METRICS = REPO_ROOT / "art_pipeline/previews/wyvern/art_c1/art_c1_metrics.json"

# Jade palette (base reference from assets/creatures/wyvern.json)
PALETTE_JADE = {
    "Scales": (0.10, 0.38, 0.29, 1.0),
    "Horns": (0.66, 0.58, 0.39, 1.0),
    "Eyes": (1.00, 0.72, 0.20, 1.0),
    "Membranes": (0.18, 0.63, 0.55, 1.0),
}

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

    # Use ShaderNodeVertexColor so glTF exporter exports COLOR_0 cleanly in MATERIAL mode
    vcol = nodes.new("ShaderNodeVertexColor")
    vcol.layer_name = "Col"
    vcol.location = (-360, 80)

    mix = nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    mix.blend_type = "MULTIPLY"
    mix.inputs["Factor"].default_value = 1.0
    mix.location = (-140, 80)

    lin = tuple(srgb_to_linear(v) for v in rgba_srgb[:3]) + (1.0,)
    mix.inputs["B"].default_value = lin

    links.new(vcol.outputs["Color"], mix.inputs["A"])
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


def normalize_weights(weight_dict, max_influences=4):
    """Clamp to top `max_influences`, drop tiny weights, and normalize strictly to 1.0."""
    cleaned = [(bone, float(w)) for bone, w in weight_dict.items() if w > 1e-6]
    cleaned.sort(key=lambda item: (-item[1], item[0]))
    cleaned = cleaned[:max_influences]
    if not cleaned:
        return {"Body": 1.0}
    total = sum(w for _, w in cleaned)
    if total <= 1e-8:
        return {"Body": 1.0}
    normed = {bone: w / total for bone, w in cleaned}
    # Fix floating-point remainder on heaviest bone
    max_bone = cleaned[0][0]
    diff = 1.0 - sum(normed.values())
    normed[max_bone] = max(0.0, min(1.0, normed[max_bone] + diff))
    return normed


def blend_weights(pairs):
    """Combine [(weight_dict, factor), ...] and normalize to <= 4 influences."""
    acc = defaultdict(float)
    for wd, factor in pairs:
        if factor <= 0.0:
            continue
        for bone, amt in wd.items():
            acc[bone] += amt * factor
    return normalize_weights(acc)


def smoothstep(edge0, edge1, x):
    if abs(edge1 - edge0) < 1e-8:
        return 1.0 if x >= edge1 else 0.0
    t = max(0.0, min(1.0, (x - edge0) / (edge1 - edge0)))
    return t * t * (3.0 - 2.0 * t)


def segment_param(pt, a, b):
    ab = b - a
    l2 = ab.length_squared
    if l2 < 1e-9:
        return 0.0, (pt - a).length
    t = max(0.0, min(1.0, (pt - a).dot(ab) / l2))
    closest = a + ab * t
    return t, (pt - closest).length


class VoxelGrid:
    """Sparse 3D voxel grid with interior culling and region-aware 2D greedy merging."""

    def __init__(self, step=0.06, min_z_world=None):
        self.step = step
        self.min_z_world = min_z_world
        # (ix, iy, iz) -> (mat_idx, shade_rgb, merge_band_id)
        self.cells = {}

    def _allowed_z(self, iz):
        if self.min_z_world is None:
            return True
        return (iz * self.step) >= (self.min_z_world - 1e-5)

    def add_world_ellipsoid(self, center, radii, mat_idx=0, shade_fn=None, band_fn=None):
        cx, cy, cz = center
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
                        shade = shade_fn(wx, wy, wz, dx, dy, dz) if shade_fn else SHADE_MID
                        band = band_fn(wx, wy, wz) if band_fn else 0
                        self.cells[(ix, iy, iz)] = (mat_idx, shade, band)

    def add_world_capsule(
        self, p0, p1, r0, r1, mat_idx=0, shade_fn=None, band_fn=None,
        flatten_x=1.0, flatten_z=1.0, keel_power=0.0
    ):
        a = Vector(p0)
        b = Vector(p1)
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
                    z_norm = delta.z / max(rad * flatten_z, 1e-5)
                    x_scale = flatten_x
                    if keel_power > 0.0 and z_norm < 0.0:
                        x_scale = max(0.32, flatten_x * (1.0 + keel_power * z_norm))
                    nx = delta.x / max(rad * x_scale, 1e-5)
                    ny = delta.y / max(rad, 1e-5)
                    nz = z_norm
                    if nx * nx + ny * ny + nz * nz <= 1.0:
                        shade = shade_fn(wx, wy, wz, t, nz) if shade_fn else SHADE_MID
                        band = band_fn(wx, wy, wz) if band_fn else 0
                        self.cells[(ix, iy, iz)] = (mat_idx, shade, band)

    def add_world_box(self, bmin, bmax, mat_idx=0, shade=SHADE_MID, band=0):
        s = self.step
        x0 = math.floor(bmin[0] / s + 1e-6)
        x1 = math.ceil(bmax[0] / s - 1e-6) - 1
        y0 = math.floor(bmin[1] / s + 1e-6)
        y1 = math.ceil(bmax[1] / s - 1e-6) - 1
        z0 = math.floor(bmin[2] / s + 1e-6)
        z1 = math.ceil(bmax[2] / s - 1e-6) - 1
        for ix in range(x0, x1 + 1):
            for iy in range(y0, y1 + 1):
                for iz in range(z0, z1 + 1):
                    if not self._allowed_z(iz):
                        continue
                    self.cells[(ix, iy, iz)] = (mat_idx, shade, band)

    def emit_into_bmesh(self, bm, color_layer, weight_fn, vert_weights_out):
        """Emit exposed, band-aware greedy-merged triangulated faces into shared `bm`."""
        s = self.step
        dirs = [
            (0, +1, 1, 2, False),
            (0, -1, 1, 2, True),
            (1, +1, 0, 2, True),
            (1, -1, 0, 2, False),
            (2, +1, 0, 1, False),
            (2, -1, 0, 1, True),
        ]

        # Local vertex cache within this grid avoids internal T-split duplicates
        local_verts = {}

        def get_vert(coords):
            key = (round(coords[0], 5), round(coords[1], 5), round(coords[2], 5))
            v = local_verts.get(key)
            if v is None:
                v = bm.verts.new(coords)
                local_verts[key] = v
                vert_weights_out[v] = normalize_weights(weight_fn(Vector(coords)))
            return v

        for axis, sign, u_ax, v_ax, flip in dirs:
            slices = {}
            for coord, val in self.cells.items():
                nbr = list(coord)
                nbr[axis] += sign
                if tuple(nbr) in self.cells:
                    continue
                k_slice = coord[axis]
                u, v = coord[u_ax], coord[v_ax]
                mat_idx, rgb, band = val
                qrgb = (round(rgb[0], 2), round(rgb[1], 2), round(rgb[2], 2))
                slices.setdefault(k_slice, {})[(u, v)] = (mat_idx, qrgb, band)

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
                        verts.append(get_vert(tuple(p)))

                    mat_idx, qrgb, _ = token
                    rgba = (qrgb[0], qrgb[1], qrgb[2], 1.0)
                    # Emit 2 crisp triangles directly
                    for tri_verts in ((verts[0], verts[1], verts[2]), (verts[0], verts[2], verts[3])):
                        try:
                            face = bm.faces.new(tri_verts)
                        except ValueError:
                            continue
                        face.material_index = mat_idx
                        face.smooth = False
                        for loop in face.loops:
                            loop[color_layer] = rgba


def emit_unified_voxel_membrane(bm, color_layer, mat_idx_mem, anchors_w, bays, vert_weights_out, voxel_step=0.055):
    """Emit a single watertight 7-bay voxel-stepped membrane per wing with exact barycentric weights.

    `anchors_w`: dict key -> (Vector_world, weight_dict)
    `bays`: list of (key_pivot, key_a, key_b, scallop_amount)
    """
    rows = 5
    cols = 6

    def qstep(val, step):
        return round(val / step) * step

    # Cache vertices by quantized spatial position or exact edge token so shared edges share vertices & weights
    shared_verts = {}

    def make_vert(pt, w_dict, shared_key=None):
        if shared_key is not None and shared_key in shared_verts:
            return shared_verts[shared_key]
        v = bm.verts.new(pt)
        vert_weights_out[v] = normalize_weights(w_dict)
        if shared_key is not None:
            shared_verts[shared_key] = v
        return v

    for key_p, key_a, key_b, scallop in bays:
        p0, w0 = anchors_w[key_p]
        pa, wa = anchors_w[key_a]
        pb, wb = anchors_w[key_b]

        grid = {}
        for r in range(rows + 1):
            t_r = r / rows
            for c in range(cols + 1):
                t_c = c / cols
                # Exact barycentric coordinates for (p0, pa, pb)
                b_a = t_r * (1.0 - t_c)
                b_b = t_r * t_c
                b_0 = 1.0 - b_a - b_b
                pt = p0 * b_0 + pa * b_a + pb * b_b
                w_dict = blend_weights([(w0, b_0), (wa, b_a), (wb, b_b)])

                if r > 0 and scallop > 0.0:
                    notch = 1.0 - abs(2.0 * t_c - 1.0)
                    notch_q = math.floor(notch * 3.0 + 0.5) / 3.0
                    inward = notch_q * (t_r ** 1.9) * scallop
                    vec = p0 - pa.lerp(pb, t_c)
                    if vec.length > 1e-5:
                        pt = pt + vec.normalized() * inward

                # Quantize interior and trailing-edge points; keep shared corner/spar edges exact for seam locking
                if r == 0:
                    skey = ("CORNER", key_p)
                elif r == rows and c == 0:
                    skey = ("CORNER", key_a)
                elif r == rows and c == cols:
                    skey = ("CORNER", key_b)
                elif c == 0:
                    skey = ("EDGE", tuple(sorted((key_p, key_a))), r if key_p < key_a else rows - r)
                elif c == cols:
                    skey = ("EDGE", tuple(sorted((key_p, key_b))), r if key_p < key_b else rows - r)
                else:
                    skey = None
                    pt.x = qstep(pt.x, voxel_step)
                    pt.y = qstep(pt.y, voxel_step)
                    pt.z = qstep(pt.z, voxel_step * 0.8)

                if 0 < r < rows:
                    camber = (0.042 if r in (2, 3) else 0.022) * (1.0 - abs(2.0 * t_c - 1.0) * 0.5)
                    pt = pt + Vector((0.0, 0.0, camber))

                grid[(r, c)] = make_vert(pt, w_dict, shared_key=skey)

        for r in range(rows):
            for c in range(cols):
                v00 = grid[(r, c)]
                v10 = grid[(r + 1, c)]
                v11 = grid[(r + 1, c + 1)]
                v01 = grid[(r, c + 1)]
                rib_shade = 0.74 if c in (0, cols - 1) else (0.95 if c in (2, 3) else 0.85)
                row_grad = 0.88 + 0.14 * (r / rows)
                if r == rows - 1:
                    row_grad *= 0.82
                s_val = min(1.05, rib_shade * row_grad)
                col = (min(1.0, s_val * 0.96), min(1.0, s_val * 0.94), min(1.0, s_val * 0.90), 1.0)
                for tri_verts in ((v00, v10, v11), (v00, v11, v01)):
                    if len({tri_verts[0], tri_verts[1], tri_verts[2]}) < 3:
                        continue
                    try:
                        f = bm.faces.new(tri_verts)
                        f.material_index = mat_idx_mem
                        f.smooth = False
                        for loop in f.loops:
                            loop[color_layer] = col
                    except ValueError:
                        pass


def define_skeleton_points():
    """Return the exact 23 joint world positions in Blender space (+Z up, -Y snout, +X left)."""
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

    hierarchy = [
        ("Root", None),
        ("Body", "Root"),
        ("NeckBase", "Body"),
        ("NeckTip", "NeckBase"),
        ("Head", "NeckTip"),
        ("Jaw", "Head"),
        ("TailBase", "Body"),
        ("TailMid", "TailBase"),
        ("TailTip", "TailMid"),
    ]
    for side in ("L", "R"):
        hierarchy.extend([
            (f"Hip.{side}", "Body"),
            (f"Knee.{side}", f"Hip.{side}"),
            (f"Ankle.{side}", f"Knee.{side}"),
            (f"WingShoulder.{side}", "Body"),
            (f"WingElbow.{side}", f"WingShoulder.{side}"),
            (f"WingWrist.{side}", f"WingElbow.{side}"),
            (f"WingDigit.{side}", f"WingWrist.{side}"),
        ])
    return pts, hierarchy


def create_armature(scene, collection, pts, hierarchy):
    """Create the 23-bone hierarchical Blender Armature aligned cleanly for Y-up glTF export."""
    arm_data = bpy.data.armatures.new("Wyvern_Armature")
    arm_data.display_type = "STICK"
    arm_obj = bpy.data.objects.new("Wyvern_Rig", arm_data)
    collection.objects.link(arm_obj)

    bpy.context.window.scene = scene
    bpy.context.view_layer.objects.active = arm_obj
    bpy.ops.object.mode_set(mode="EDIT")

    ebones = {}
    for name, parent_name in hierarchy:
        eb = arm_data.edit_bones.new(name)
        pos = Vector(pts[name])
        eb.head = pos
        # Point +Z in Blender (+Y in glTF) and roll-align Z to -Y (+Z in glTF) -> identity bind rotation in glTF!
        eb.tail = pos + Vector((0.0, 0.0, 0.22))
        eb.align_roll(Vector((0.0, -1.0, 0.0)))
        eb.use_connect = False
        eb.use_deform = True
        if parent_name is not None:
            eb.parent = ebones[parent_name]
        ebones[name] = eb

    bpy.ops.object.mode_set(mode="OBJECT")
    arm_obj.location = (0.0, 0.0, 0.0)
    arm_obj.rotation_euler = (0.0, 0.0, 0.0)
    arm_obj.scale = (1.0, 1.0, 1.0)
    return arm_obj


def build_unified_skinned_mesh(collection, arm_obj, pts, hierarchy, mats):
    """Build ONE unified voxel + membrane mesh skinned to all deforming joints."""
    SCALES, HORNS, EYES, MEMBRANES = 0, 1, 2, 3
    bm = bmesh.new()
    color_layer = bm.loops.layers.color.new("Col")
    vert_weights = {}  # BMVert -> {bone_name: weight}

    def belly_shade(wx, wy, wz, t, nz):
        if nz < -0.32:
            return SHADE_BELLY
        if nz > 0.56:
            return SHADE_DORSAL
        return SHADE_MID

    p_body = Vector(pts["Body"])
    p_nb = Vector(pts["NeckBase"])
    p_nt = Vector(pts["NeckTip"])
    p_head = Vector(pts["Head"])
    p_tb = Vector(pts["TailBase"])
    p_tm = Vector(pts["TailMid"])
    p_tt = Vector(pts["TailTip"])
    p_tail_end = Vector((-0.18, 3.68, 1.02))

    # =========================================================================
    # 1. CONTINUOUS SPINE: BODY + NECK + TAIL (seamless voxel grid at step=0.062)
    # =========================================================================
    g_spine = VoxelGrid(step=0.062)

    def spine_band(wx, wy, wz):
        # Slice greedy-merge bands every ~0.18 m along flexible neck/tail zones; keep rigid body torso merged
        if wy < -0.35 or wy > 0.50:
            return int(round(wy / 0.16))
        return 0

    def spine_weight(pt):
        # Check neck progression (-Y)
        if pt.y < -0.32:
            if pt.y >= p_nb.y:
                u = smoothstep(-0.32, p_nb.y, pt.y)
                return {"Body": 1.0 - 0.65 * u, "NeckBase": 0.65 * u}
            if pt.y >= p_nt.y:
                u = smoothstep(p_nb.y, p_nt.y, pt.y)
                return {"NeckBase": 1.0 - u, "NeckTip": u}
            u = smoothstep(p_nt.y, p_head.y, pt.y)
            return {"NeckTip": 1.0 - 0.85 * u, "Head": 0.85 * u}

        # Check tail progression (+Y)
        if pt.y > 0.48:
            if pt.y <= p_tb.y:
                u = smoothstep(0.48, p_tb.y, pt.y)
                return {"Body": 1.0 - 0.65 * u, "TailBase": 0.65 * u}
            if pt.y <= p_tm.y:
                u = smoothstep(p_tb.y, p_tm.y, pt.y)
                return {"TailBase": 1.0 - u, "TailMid": u}
            if pt.y <= p_tt.y:
                u = smoothstep(p_tm.y, p_tt.y, pt.y)
                return {"TailMid": 1.0 - u, "TailTip": u}
            return {"TailTip": 1.0}

        # Torso socket blends near shoulders and hips for smooth limb motion
        w = {"Body": 1.0}
        if abs(pt.x) > 0.24:
            side = "L" if pt.x > 0 else "R"
            ws_p = Vector(pts[f"WingShoulder.{side}"])
            hp_p = Vector(pts[f"Hip.{side}"])
            d_ws = (pt - ws_p).length
            d_hp = (pt - hp_p).length
            if d_ws < 0.34:
                f = 0.42 * (1.0 - smoothstep(0.08, 0.34, d_ws))
                w = {"Body": 1.0 - f, f"WingShoulder.{side}": f}
            elif d_hp < 0.32:
                f = 0.40 * (1.0 - smoothstep(0.08, 0.32, d_hp))
                w = {"Body": 1.0 - f, f"Hip.{side}": f}
        return w

    # Torso (taut V-keel sternum -> tucked waist -> pelvis)
    g_spine.add_world_capsule((0, 0.84, 1.20), (0, 0.30, 1.26), 0.30, 0.38, SCALES, belly_shade, spine_band, flatten_x=0.90, flatten_z=0.86, keel_power=0.38)
    g_spine.add_world_capsule((0, 0.30, 1.26), (0, -0.34, 1.38), 0.38, 0.50, SCALES, belly_shade, spine_band, flatten_x=0.92, flatten_z=0.96, keel_power=0.52)
    g_spine.add_world_capsule((0, -0.34, 1.38), (0, -0.68, 1.48), 0.48, 0.31, SCALES, belly_shade, spine_band, flatten_x=0.92, flatten_z=0.92, keel_power=0.45)
    g_spine.add_world_capsule((0, -0.54, 1.26), (0, 0.04, 1.06), 0.16, 0.24, SCALES, lambda *a: SHADE_BELLY, spine_band, flatten_x=0.58, flatten_z=1.10)
    for sign in (1.0, -1.0):
        g_spine.add_world_capsule((0.28 * sign, -0.40, 1.56), (0.36 * sign, -0.05, 1.62), 0.14, 0.11, SCALES, lambda *a: SHADE_DORSAL, spine_band)

    # Continuous Neck S-curve (Body -> NeckBase -> NeckTip -> Head socket)
    g_spine.add_world_capsule((0, -0.46, 1.42), (0, -1.06, 1.88), 0.31, 0.23, SCALES, belly_shade, spine_band, flatten_x=0.84, flatten_z=0.98, keel_power=0.35)
    g_spine.add_world_capsule((0, -1.02, 1.86), (0, -1.62, 2.22), 0.23, 0.20, SCALES, belly_shade, spine_band, flatten_x=0.82, flatten_z=0.96, keel_power=0.30)

    # Continuous Tail (Body -> TailBase -> TailMid -> TailTip -> spade tip)
    g_spine.add_world_capsule((0, 0.66, 1.21), (0, 1.70, 1.03), 0.29, 0.19, SCALES, belly_shade, spine_band, keel_power=0.25)
    g_spine.add_world_capsule((0.0, 1.66, 1.04), (0.06, 2.60, 0.80), 0.19, 0.11, SCALES, belly_shade, spine_band)
    g_spine.add_world_capsule((0.06, 2.56, 0.81), (0.01, 3.12, 0.84), 0.11, 0.065, SCALES, belly_shade, spine_band)
    g_spine.add_world_capsule((0.01, 3.12, 0.84), (-0.16, 3.63, 1.02), 0.065, 0.032, SCALES, belly_shade, spine_band)
    g_spine.add_world_ellipsoid((-0.18, 3.53, 1.02), (0.14, 0.16, 0.045), HORNS, lambda *a: SHADE_HORN_BASE, spine_band)

    # Dorsal & caudal spine plates (raised slightly above shoulder silhouette for crisp profile)
    for y_sp, z_sp, h_sp, x_sp in (
        (-0.68, 1.66, 0.22, 0.0),
        (-0.88, 1.84, 0.23, 0.0),
        (-1.10, 2.02, 0.22, 0.0),
        (-1.34, 2.20, 0.20, 0.0),
        (-0.56, 1.78, 0.33, 0.0),
        (-0.36, 1.84, 0.41, 0.0),
        (-0.14, 1.86, 0.45, 0.0),
        (0.10, 1.82, 0.41, 0.0),
        (0.34, 1.76, 0.36, 0.0),
        (0.58, 1.66, 0.30, 0.0),
        (0.90, 1.44, 0.20, 0.0),
        (1.18, 1.38, 0.19, 0.0),
        (1.46, 1.30, 0.18, 0.0),
        (1.76, 1.20, 0.16, 0.01),
        (2.06, 1.12, 0.15, 0.03),
        (2.36, 1.02, 0.14, 0.05),
        (2.74, 0.94, 0.12, 0.04),
        (3.02, 0.94, 0.11, 0.01),
    ):
        g_spine.add_world_capsule(
            (x_sp, y_sp, z_sp), (x_sp, y_sp + 0.13, z_sp + h_sp),
            0.074, 0.036, HORNS, lambda *a: SHADE_HORN_BASE, spine_band, flatten_x=0.55
        )

    g_spine.emit_into_bmesh(bm, color_layer, spine_weight, vert_weights)

    # =========================================================================
    # 2. RIGID HEAD & ARTICULATED JAW (step=0.048 for crisp eyes, fangs, horns)
    # =========================================================================
    g_head = VoxelGrid(step=0.048)
    g_head.add_world_capsule(
        (0, -1.52, 2.24), (0, -1.88, 2.28), 0.28, 0.25, SCALES,
        lambda wx, wy, wz, t, nz: SHADE_DORSAL if nz > 0.25 else SHADE_MID,
        flatten_x=1.06, flatten_z=0.82, keel_power=0.2
    )
    g_head.add_world_capsule(
        (0, -1.88, 2.26), (0, -2.56, 2.20), 0.22, 0.095, SCALES,
        lambda wx, wy, wz, t, nz: SHADE_DORSAL if nz > 0.2 else SHADE_FLANK,
        flatten_x=0.95, flatten_z=0.72
    )
    g_head.add_world_box((-0.075, -2.68, 2.13), (0.075, -2.54, 2.25), SCALES, SHADE_DARK)
    g_head.add_world_capsule((0, -2.02, 2.36), (0, -2.52, 2.28), 0.065, 0.035, SCALES, lambda *a: SHADE_DORSAL, flatten_x=0.6)
    for sign in (1.0, -1.0):
        g_head.add_world_capsule((0.24 * sign, -1.66, 2.40), (0.21 * sign, -2.06, 2.35), 0.082, 0.050, SCALES, lambda *a: SHADE_DARK)
        g_head.add_world_capsule((0.20 * sign, -1.68, 2.38), (0.29 * sign, -1.52, 2.50), 0.135, 0.115, SCALES, lambda *a: SHADE_DORSAL)
        g_head.add_world_capsule((0.27 * sign, -1.62, 2.17), (0.18 * sign, -2.14, 2.15), 0.085, 0.045, SCALES, lambda *a: SHADE_FLANK)
        g_head.add_world_box((0.25 * sign - 0.04, -1.96, 2.26), (0.25 * sign + 0.04, -1.80, 2.36), SCALES, SHADE_EYE_SOCKET)
        g_head.add_world_box((0.27 * sign - 0.04, -1.92, 2.28), (0.27 * sign + 0.04, -1.84, 2.34), EYES, (1.0, 1.0, 1.0))
        horn_tip = (0.49 * sign, -1.04 if sign > 0 else -1.22, 2.815 if sign > 0 else 2.69)
        g_head.add_world_capsule((0.25 * sign, -1.58, 2.46), (0.38 * sign, -1.34, 2.64), 0.11, 0.068, HORNS, lambda *a: SHADE_HORN_BASE)
        g_head.add_world_capsule((0.38 * sign, -1.34, 2.64), horn_tip, 0.068, 0.024 if sign > 0 else 0.042, HORNS, lambda *a: SHADE_HORN_TIP)
        g_head.add_world_capsule((0.24 * sign, -1.62, 2.22), (0.42 * sign, -1.24, 2.29), 0.068, 0.022, HORNS, lambda *a: SHADE_HORN_BASE)
        g_head.add_world_box((0.11 * sign - 0.025, -2.44, 2.08), (0.11 * sign + 0.025, -2.37, 2.16), HORNS, SHADE_HORN_TIP)
        g_head.add_world_box((0.15 * sign - 0.025, -2.24, 2.10), (0.15 * sign + 0.025, -2.17, 2.16), HORNS, SHADE_HORN_TIP)
    g_head.add_world_box((-0.24, -2.20, 2.15), (-0.18, -2.04, 2.19), SCALES, SHADE_DARK)
    g_head.emit_into_bmesh(
        bm, color_layer,
        lambda pt: {"Head": 1.0} if (pt.y < -1.56 or pt.z > 2.38) else {"Head": 0.85, "NeckTip": 0.15},
        vert_weights,
    )

    g_jaw = VoxelGrid(step=0.048)
    g_jaw.add_world_capsule((0, -1.64, 2.04), (0, -2.52, 2.07), 0.22, 0.088, SCALES, lambda *a: SHADE_BELLY, flatten_x=0.98, flatten_z=0.55)
    g_jaw.add_world_capsule((0, -1.78, 1.98), (0, -1.62, 1.88), 0.05, 0.022, HORNS, lambda *a: SHADE_HORN_BASE)
    for sign in (1.0, -1.0):
        g_jaw.add_world_box((0.10 * sign - 0.025, -2.42, 2.09), (0.10 * sign + 0.025, -2.35, 2.15), HORNS, SHADE_HORN_TIP)
    g_jaw.emit_into_bmesh(bm, color_layer, lambda pt: {"Jaw": 1.0}, vert_weights)

    # =========================================================================
    # 3. CONTINUOUS HIND LEGS (L and R, seamless thigh -> knee -> shin -> foot)
    # =========================================================================
    for side, sign in (("L", 1.0), ("R", -1.0)):
        h_pt = Vector(pts[f"Hip.{side}"])
        k_pt = Vector(pts[f"Knee.{side}"])
        a_pt = Vector(pts[f"Ankle.{side}"])
        f_pt = Vector((0.60 * sign, 0.54, 0.09))

        def leg_band(wx, wy, wz):
            return int(round(wz / 0.15))

        def leg_weight(pt, side=side, h_pt=h_pt, k_pt=k_pt, a_pt=a_pt, f_pt=f_pt):
            if pt.z <= 0.16:
                return {f"Ankle.{side}": 1.0}
            t_hk, d_hk = segment_param(pt, h_pt, k_pt)
            t_ka, d_ka = segment_param(pt, k_pt, a_pt)
            if d_hk <= d_ka:
                if t_hk < 0.25 and pt.z > 1.08:
                    u = smoothstep(0.0, 0.25, t_hk)
                    return {"Body": 0.35 * (1.0 - u), f"Hip.{side}": 0.65 + 0.35 * u}
                if t_hk > 0.72:
                    u = smoothstep(0.72, 1.0, t_hk)
                    return {f"Hip.{side}": 1.0 - 0.50 * u, f"Knee.{side}": 0.50 * u}
                return {f"Hip.{side}": 1.0}
            else:
                if t_ka < 0.28:
                    u = smoothstep(0.0, 0.28, t_ka)
                    return {f"Hip.{side}": 0.50 * (1.0 - u), f"Knee.{side}": 0.50 + 0.50 * u}
                if t_ka > 0.72:
                    u = smoothstep(0.72, 1.0, t_ka)
                    return {f"Knee.{side}": 1.0 - 0.50 * u, f"Ankle.{side}": 0.50 * u}
                return {f"Knee.{side}": 1.0}

        g_leg = VoxelGrid(step=0.052, min_z_world=0.0)
        g_leg.add_world_capsule((0.26 * sign, 0.58, 1.24), h_pt, 0.23, 0.27, SCALES, belly_shade, leg_band)
        g_leg.add_world_capsule(h_pt, k_pt, 0.26, 0.16, SCALES, belly_shade, leg_band)
        g_leg.add_world_capsule(k_pt, a_pt, 0.155, 0.105, SCALES, belly_shade, leg_band)
        g_leg.add_world_capsule(a_pt, f_pt, 0.10, 0.105, SCALES, belly_shade, leg_band)
        g_leg.add_world_box((0.60 * sign - 0.14, 0.44, 0.00), (0.60 * sign + 0.14, 0.70, 0.12), SCALES, SHADE_BELLY)
        for ti in (-1, 0, 1):
            tx = 0.60 * sign + ti * 0.11
            ty_end = 0.24 - (0.06 if ti == 0 else 0.0)
            g_leg.add_world_capsule((0.60 * sign + ti * 0.06, 0.48, 0.06), (tx, ty_end, 0.05), 0.055, 0.04, SCALES, lambda *a: SHADE_MID)
            g_leg.add_world_capsule((tx, ty_end, 0.05), (tx, ty_end - 0.11, 0.02), 0.04, 0.02, HORNS, lambda *a: SHADE_HORN_BASE)
        g_leg.add_world_capsule((0.60 * sign, 0.72, 0.10), (0.60 * sign, 0.88, 0.06), 0.045, 0.02, HORNS, lambda *a: SHADE_HORN_BASE)
        g_leg.emit_into_bmesh(bm, color_layer, leg_weight, vert_weights)

    # =========================================================================
    # 4. CONTINUOUS WINGS + UNIFIED 7-BAY DEFORMABLE MEMBRANES
    # =========================================================================
    for side, sign in (("L", 1.0), ("R", -1.0)):
        ws = Vector(pts[f"WingShoulder.{side}"])
        we = Vector(pts[f"WingElbow.{side}"])
        ww = Vector(pts[f"WingWrist.{side}"])
        wd = Vector(pts[f"WingDigit.{side}"])
        tip_outer = Vector((3.55 * sign, 0.36, 1.25))
        f2_tip = Vector((3.15 * sign, 1.25, 1.19))
        f3_tip = Vector((2.48 * sign, 1.68, 1.11))
        f4_tip = Vector((1.60 * sign, 1.64, 1.08))
        flank = Vector((0.42 * sign, 0.64, 1.15))

        w_ws = {f"WingShoulder.{side}": 0.78, "Body": 0.22}
        w_we = {f"WingElbow.{side}": 0.68, f"WingShoulder.{side}": 0.32}
        w_ww = {f"WingWrist.{side}": 1.0}
        w_wd = {f"WingDigit.{side}": 0.65, f"WingWrist.{side}": 0.35}
        w_tip = {f"WingDigit.{side}": 1.0}
        w_f2 = {f"WingDigit.{side}": 0.65, f"WingWrist.{side}": 0.35}
        w_f3 = {f"WingDigit.{side}": 0.25, f"WingWrist.{side}": 0.75}
        w_f4 = {f"WingWrist.{side}": 0.82, f"WingElbow.{side}": 0.18}
        w_flank = {"Body": 0.88, f"WingShoulder.{side}": 0.12}

        def wing_band(wx, wy, wz):
            return int(round(abs(wx) / 0.16)) * 100 + int(round(wy / 0.22))

        def wing_weight(
            pt, side=side, sign=sign, ws=ws, we=we, ww=ww, wd=wd,
            tip_outer=tip_outer, f2_tip=f2_tip, f3_tip=f3_tip, f4_tip=f4_tip,
            w_f2=w_f2, w_f3=w_f3, w_f4=w_f4
        ):
            # Secondary finger spars: check closest spar behind wrist
            if pt.y > ww.y + 0.12 and (pt - wd).length > 0.25:
                t2, d2 = segment_param(pt, ww, f2_tip)
                t3, d3 = segment_param(pt, ww, f3_tip)
                t4, d4 = segment_param(pt, ww, f4_tip)
                best_t, _, best_w = min(
                    (t2, d2, w_f2), (t3, d3, w_f3), (t4, d4, w_f4),
                    key=lambda item: item[1],
                )
                return blend_weights([({f"WingWrist.{side}": 1.0}, 1.0 - best_t), (best_w, best_t)])

            # Main leading arm chain: Body -> Shoulder -> Elbow -> Wrist -> Digit -> Tip
            t_se, d_se = segment_param(pt, ws, we)
            t_ew, d_ew = segment_param(pt, we, ww)
            t_wd, d_wd = segment_param(pt, ww, wd)
            t_dt, d_dt = segment_param(pt, wd, tip_outer)

            if d_se <= min(d_ew, d_wd, d_dt):
                if t_se < 0.20 and abs(pt.x) < 0.56:
                    u = smoothstep(0.0, 0.20, t_se)
                    return {"Body": 0.45 * (1.0 - u), f"WingShoulder.{side}": 0.55 + 0.45 * u}
                if t_se > 0.72:
                    u = smoothstep(0.72, 1.0, t_se)
                    return {f"WingShoulder.{side}": 1.0 - 0.50 * u, f"WingElbow.{side}": 0.50 * u}
                return {f"WingShoulder.{side}": 1.0}
            if d_ew <= min(d_wd, d_dt):
                if t_ew < 0.28:
                    u = smoothstep(0.0, 0.28, t_ew)
                    return {f"WingShoulder.{side}": 0.50 * (1.0 - u), f"WingElbow.{side}": 0.50 + 0.50 * u}
                if t_ew > 0.72:
                    u = smoothstep(0.72, 1.0, t_ew)
                    return {f"WingElbow.{side}": 1.0 - 0.50 * u, f"WingWrist.{side}": 0.50 * u}
                return {f"WingElbow.{side}": 1.0}
            if d_wd <= d_dt:
                if t_wd < 0.28:
                    u = smoothstep(0.0, 0.28, t_wd)
                    return {f"WingElbow.{side}": 0.50 * (1.0 - u), f"WingWrist.{side}": 0.50 + 0.50 * u}
                if t_wd > 0.72:
                    u = smoothstep(0.72, 1.0, t_wd)
                    return {f"WingWrist.{side}": 1.0 - 0.50 * u, f"WingDigit.{side}": 0.50 * u}
                return {f"WingWrist.{side}": 1.0}
            if t_dt < 0.24:
                u = smoothstep(0.0, 0.24, t_dt)
                return {f"WingWrist.{side}": 0.50 * (1.0 - u), f"WingDigit.{side}": 0.50 + 0.50 * u}
            return {f"WingDigit.{side}": 1.0}

        g_wing = VoxelGrid(step=0.048)
        g_wing.add_world_capsule((0.26 * sign, -0.40, 1.45), ws, 0.21, 0.19, SCALES, belly_shade, wing_band)
        g_wing.add_world_capsule(ws, we, 0.18, 0.125, SCALES, belly_shade, wing_band)
        g_wing.add_world_capsule(we, ww, 0.125, 0.095, SCALES, belly_shade, wing_band)
        g_wing.add_world_capsule(ww, wd, 0.095, 0.055, SCALES, belly_shade, wing_band)
        g_wing.add_world_ellipsoid(ww, (0.11, 0.11, 0.10), SCALES, lambda *a: SHADE_DORSAL, wing_band)
        thumb_mid = (2.06 * sign, -1.10, 1.89)
        thumb_tip = (1.96 * sign, -1.24, 1.81)
        g_wing.add_world_capsule(ww, thumb_mid, 0.068, 0.042, HORNS, lambda *a: SHADE_HORN_BASE, wing_band)
        g_wing.add_world_capsule(thumb_mid, thumb_tip, 0.042, 0.02, HORNS, lambda *a: SHADE_HORN_TIP, wing_band)
        for ft in (f2_tip, f3_tip, f4_tip):
            g_wing.add_world_capsule(ww, ft, 0.052, 0.026, SCALES, lambda *a: SHADE_DORSAL, wing_band)
        g_wing.add_world_capsule(wd, tip_outer, 0.058, 0.028, SCALES, lambda *a: SHADE_DORSAL, wing_band)
        g_wing.add_world_box(
            (min(3.55 * sign, 3.63 * sign), 0.33, 1.22),
            (max(3.55 * sign, 3.63 * sign), 0.40, 1.28),
            HORNS, SHADE_HORN_BASE, band=9999 * int(sign),
        )
        g_wing.emit_into_bmesh(bm, color_layer, wing_weight, vert_weights)

        # Single unified 7-bay deformable membrane per wing
        mem_tip_anchor = Vector((3.52 * sign, 0.36, 1.25))
        anchors_w = {
            "ws": (ws, w_ws),
            "we": (we, w_we),
            "ww": (ww, w_ww),
            "wd": (wd, w_wd),
            "tip": (mem_tip_anchor, w_tip),
            "f2": (f2_tip, w_f2),
            "f3": (f3_tip, w_f3),
            "f4": (f4_tip, w_f4),
            "flank": (flank, w_flank),
        }
        bays = [
            ("wd", "tip", "f2", 0.06),
            ("ww", "wd", "f2", 0.0),
            ("ww", "f2", "f3", 0.24),
            ("ww", "f3", "f4", 0.26),
            ("ww", "we", "ws", 0.0),
            ("ww", "ws", "flank", 0.0),
            ("ww", "flank", "f4", 0.20),
        ]
        emit_unified_voxel_membrane(bm, color_layer, MEMBRANES, anchors_w, bays, vert_weights, voxel_step=0.055)

    # Convert bmesh to Mesh, assign materials, vertex groups, and color attribute
    bm.normal_update()
    mesh = bpy.data.meshes.new("Wyvern")
    bm.to_mesh(mesh)

    # Map vertex index -> weights BEFORE freeing bm
    bm.verts.ensure_lookup_table()
    indexed_weights = [vert_weights[v] for v in bm.verts]
    bm.free()

    for mat in mats:
        mesh.materials.append(mat)

    if mesh.color_attributes:
        mesh.color_attributes.active_color = mesh.color_attributes[0]
        mesh.color_attributes.render_color_index = 0

    mesh_obj = bpy.data.objects.new("Wyvern_Mesh", mesh)
    collection.objects.link(mesh_obj)
    mesh_obj.parent = arm_obj
    mesh_obj.location = (0.0, 0.0, 0.0)
    mesh_obj.rotation_euler = (0.0, 0.0, 0.0)
    mesh_obj.scale = (1.0, 1.0, 1.0)

    # Create vertex groups for all 23 bones (Root exists as 0-weight group or omitted; keep all 23 for completeness)
    vgroups = {name: mesh_obj.vertex_groups.new(name=name) for name, _ in hierarchy}
    for v_idx, w_dict in enumerate(indexed_weights):
        for bone_name, amount in w_dict.items():
            vgroups[bone_name].add([v_idx], amount, "REPLACE")

    arm_mod = mesh_obj.modifiers.new(name="Wyvern_Armature", type="ARMATURE")
    arm_mod.object = arm_obj
    arm_mod.use_vertex_groups = True

    return mesh_obj


def audit_mesh_weights(mesh_obj, hierarchy):
    """Verify max influences <= 4, normalized sums, no NaNs, 0 unassigned vertices."""
    bone_names = {name for name, _ in hierarchy}
    group_index_to_name = {vg.index: vg.name for vg in mesh_obj.vertex_groups}
    mesh = mesh_obj.data

    max_inf = 0
    unassigned = 0
    norm_errors = 0
    invalid_values = 0
    root_assigned = 0
    bones_used = set()

    for v in mesh.vertices:
        active = []
        for g in v.groups:
            w = float(g.weight)
            if not math.isfinite(w) or w < 0.0 or w > 1.0001:
                invalid_values += 1
            if w > 1e-6:
                bname = group_index_to_name.get(g.group, "")
                if bname not in bone_names:
                    invalid_values += 1
                if bname == "Root":
                    root_assigned += 1
                active.append((bname, w))
                bones_used.add(bname)
        if not active:
            unassigned += 1
        else:
            max_inf = max(max_inf, len(active))
            s = sum(w for _, w in active)
            if abs(s - 1.0) > 1e-3:
                norm_errors += 1

    return {
        "vertex_count": len(mesh.vertices),
        "triangle_count": sum(len(p.vertices) - 2 for p in mesh.polygons),
        "max_influences_per_vertex": max_inf,
        "unassigned_vertices": unassigned,
        "normalization_errors": norm_errors,
        "invalid_weight_values": invalid_values,
        "root_weighted_vertices": root_assigned,
        "deforming_bones_used_count": len(bones_used),
        "unused_bones": sorted(bone_names - bones_used),
    }


def reset_pose(arm_obj):
    for pb in arm_obj.pose.bones:
        pb.location = (0.0, 0.0, 0.0)
        pb.rotation_mode = "QUATERNION"
        pb.rotation_quaternion = (1.0, 0.0, 0.0, 0.0)
        pb.rotation_euler = (0.0, 0.0, 0.0)
        pb.scale = (1.0, 1.0, 1.0)
    bpy.context.view_layer.update()


def aim_pose_bone(arm_obj, bone_name, target_world_dir, axial_roll=0.0):
    """Rotate `bone_name` in pose space so rest direction aligns to `target_world_dir`.

    Since all bones in this rig have identity world rotation in rest pose, a bone's rest
    direction vector is simply `(child_head - bone_head)` in world coordinates.
    """
    pb = arm_obj.pose.bones[bone_name]
    pb.rotation_mode = "QUATERNION"
    # Rest-pose child direction in world
    child = pb.children[0] if pb.children else None
    if child is not None:
        rest_dir = (child.bone.head_local - pb.bone.head_local).normalized()
    else:
        rest_dir = Vector((1.0, 0.0, 0.0))
    if pb.parent is not None:
        m_rel = pb.parent.bone.matrix_local.to_3x3().inverted() @ pb.bone.matrix_local.to_3x3()
        m_base = arm_obj.matrix_world.to_3x3() @ pb.parent.matrix.to_3x3() @ m_rel
    else:
        m_base = arm_obj.matrix_world.to_3x3() @ pb.bone.matrix_local.to_3x3()
    bone_rest_dir = (pb.bone.matrix_local.to_3x3().inverted() @ rest_dir).normalized()
    bone_desired_dir = (m_base.inverted() @ target_world_dir).normalized()
    q = bone_rest_dir.rotation_difference(bone_desired_dir)
    if abs(axial_roll) > 1e-5:
        q = Quaternion(bone_desired_dir, axial_roll) @ q
    pb.rotation_quaternion = q
    bpy.context.view_layer.update()


def apply_test_pose(arm_obj, pose_name, pts):
    """Apply one of the technical test poses strictly via Armature PoseBones (no mesh rebuild)."""
    reset_pose(arm_obj)
    if pose_name == "BindOpen":
        return

    def set_rot(bname, rx, ry, rz):
        # Convert desired world-axes Euler (Blender +X/+Y/+Z) into bone-local quaternion
        # Bone local axes at rest: bone_X = world_+X, bone_Y = world_+Z, bone_Z = world_-Y
        pb = arm_obj.pose.bones[bname]
        pb.rotation_mode = "QUATERNION"
        world_rot = Euler((rx, ry, rz), "XYZ").to_matrix()
        bmat = pb.bone.matrix_local.to_3x3()
        local_rot = bmat.inverted() @ world_rot @ bmat
        pb.rotation_quaternion = local_rot.to_quaternion()

    if pose_name == "FoldedWings":
        # Subtle crouch balanced so feet stay at Z ~ 0.00 m
        set_rot("Body", 0.03, 0.0, 0.0)
        set_rot("NeckBase", -0.04, 0.0, 0.0)
        set_rot("NeckTip", 0.04, 0.0, 0.0)
        for side, sign in (("L", 1.0), ("R", -1.0)):
            set_rot(f"Hip.{side}", -0.05, 0.0, 0.0)
            set_rot(f"Knee.{side}", 0.08, 0.0, 0.0)
            set_rot(f"Ankle.{side}", -0.06, 0.0, 0.0)
        bpy.context.view_layer.update()

        for side, sign in (("L", 1.0), ("R", -1.0)):
            # 1. Shoulder -> Elbow tucked cleanly outside flank/hip
            ws_pos = arm_obj.matrix_world @ arm_obj.pose.bones[f"WingShoulder.{side}"].head
            aim_pose_bone(arm_obj, f"WingShoulder.{side}", Vector((1.06 * sign, 0.28, 1.66)) - ws_pos)
            # 2. Elbow -> Wrist folded forward & elevated
            we_pos = arm_obj.matrix_world @ arm_obj.pose.bones[f"WingElbow.{side}"].head
            aim_pose_bone(arm_obj, f"WingElbow.{side}", Vector((1.36 * sign, -0.40, 1.98)) - we_pos, axial_roll=-0.12 * sign)
            # 3. Wrist -> Digit swept backward & elevated (~3.53 m width, Z_min >> 0.20 m)
            ww_pos = arm_obj.matrix_world @ arm_obj.pose.bones[f"WingWrist.{side}"].head
            aim_pose_bone(arm_obj, f"WingWrist.{side}", Vector((1.74 * sign, 1.18, 1.90)) - ww_pos, axial_roll=-0.22 * sign)
            # 4. Gentle inward tuck on WingDigit (spread stays coherent with membrane)
            set_rot(f"WingDigit.{side}", 0.02, 0.0, 0.14 * sign)
        bpy.context.view_layer.update()
        return

    if pose_name == "NeckHeadFlex":
        set_rot("NeckBase", 0.22, 0.0, 0.24)
        set_rot("NeckTip", -0.18, 0.0, 0.28)
        set_rot("Head", -0.16, 0.10, 0.22)
        bpy.context.view_layer.update()
        return

    if pose_name == "JawOpen":
        set_rot("Jaw", 0.42, 0.0, 0.0)
        bpy.context.view_layer.update()
        return

    if pose_name == "LegsFlex":
        set_rot("Body", 0.08, 0.0, 0.0)
        set_rot("Hip.L", -0.35, 0.0, 0.0)
        set_rot("Knee.L", 0.55, 0.0, 0.0)
        set_rot("Ankle.L", -0.28, 0.0, 0.0)
        set_rot("Hip.R", 0.28, 0.0, 0.0)
        set_rot("Knee.R", 0.30, 0.0, 0.0)
        set_rot("Ankle.R", -0.40, 0.0, 0.0)
        bpy.context.view_layer.update()
        return

    if pose_name == "TailCurl":
        set_rot("TailBase", -0.12, 0.0, 0.34)
        set_rot("TailMid", -0.18, 0.0, 0.45)
        set_rot("TailTip", -0.20, 0.0, 0.48)
        bpy.context.view_layer.update()
        return

    if pose_name == "ArticulationCombined":
        # Stress test combining neck/head/jaw/legs/tail + half-folded wings
        set_rot("Body", 0.05, 0.0, 0.0)
        set_rot("NeckBase", 0.18, 0.0, 0.20)
        set_rot("NeckTip", -0.15, 0.0, 0.24)
        set_rot("Head", -0.14, 0.08, 0.18)
        set_rot("Jaw", 0.40, 0.0, 0.0)
        set_rot("TailBase", -0.10, 0.0, 0.32)
        set_rot("TailMid", -0.16, 0.0, 0.42)
        set_rot("TailTip", -0.18, 0.0, 0.46)
        set_rot("Hip.L", -0.30, 0.0, 0.0)
        set_rot("Knee.L", 0.50, 0.0, 0.0)
        set_rot("Ankle.L", -0.25, 0.0, 0.0)
        set_rot("Hip.R", 0.24, 0.0, 0.0)
        set_rot("Knee.R", 0.28, 0.0, 0.0)
        set_rot("Ankle.R", -0.35, 0.0, 0.0)
        for side, sign in (("L", 1.0), ("R", -1.0)):
            set_rot(f"WingShoulder.{side}", -0.22, 0.0, 0.35 * sign)
            set_rot(f"WingElbow.{side}", 0.05, 0.0, -0.30 * sign)
            set_rot(f"WingWrist.{side}", 0.0, 0.0, 0.20 * sign)
        bpy.context.view_layer.update()
        return

    raise ValueError(f"Unknown test pose: {pose_name}")


def measure_deformed_mesh(mesh_obj):
    """Measure evaluated world-space vertex positions, bounds, membrane ground clearance, and inverted faces."""
    depsgraph = bpy.context.evaluated_depsgraph_get()
    eval_obj = mesh_obj.evaluated_get(depsgraph)
    me = eval_obj.to_mesh()
    me.calc_loop_triangles()

    mem_idx = next((i for i, m in enumerate(mesh_obj.data.materials) if m and m.name == "Membranes"), 3)
    min_pt = Vector((float("inf"), float("inf"), float("inf")))
    max_pt = Vector((float("-inf"), float("-inf"), float("-inf")))
    mem_min_z = float("inf")
    zero_area_tris = 0

    coords = []
    mat_w = mesh_obj.matrix_world
    for v in me.vertices:
        wp = mat_w @ v.co
        coords.append((wp.x, wp.y, wp.z))
        min_pt.x = min(min_pt.x, wp.x)
        min_pt.y = min(min_pt.y, wp.y)
        min_pt.z = min(min_pt.z, wp.z)
        max_pt.x = max(max_pt.x, wp.x)
        max_pt.y = max(max_pt.y, wp.y)
        max_pt.z = max(max_pt.z, wp.z)

    for poly in me.polygons:
        if poly.area <= 1e-10:
            zero_area_tris += 1
        if poly.material_index == mem_idx:
            for vi in poly.vertices:
                mem_min_z = min(mem_min_z, coords[vi][2])

    tris = len(me.loop_triangles)
    verts = len(me.vertices)
    eval_obj.to_mesh_clear()

    dims = max_pt - min_pt
    return {
        "vertices": verts,
        "triangles": tris,
        "bounds_min_m": [round(min_pt.x, 4), round(min_pt.y, 4), round(min_pt.z, 4)],
        "bounds_max_m": [round(max_pt.x, 4), round(max_pt.y, 4), round(max_pt.z, 4)],
        "width_x_m": round(dims.x, 4),
        "length_y_m": round(dims.y, 4),
        "height_z_m": round(dims.z, 4),
        "membrane_min_z_m": round(mem_min_z, 4) if mem_min_z < float("inf") else None,
        "zero_area_polygons": zero_area_tris,
    }, coords


def create_human_reference(collection, location=(2.20, -2.35, 0.0)):
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

    bm = bmesh.new()
    cl = bm.loops.layers.color.new("Col")
    vw = {}
    g.emit_into_bmesh(bm, cl, lambda pt: {}, vw)
    mesh = bpy.data.meshes.new("Human_180cm_Mesh")
    bm.to_mesh(mesh)
    bm.free()
    mesh.materials.append(mat_human)
    obj = bpy.data.objects.new("Human_ScaleRef_180cm", mesh)
    obj.location = Vector(location)
    collection.objects.link(obj)
    return obj


def setup_diagnostic_stage(scene):
    scene.unit_settings.system = "METRIC"
    scene.unit_settings.scale_length = 1.0
    scene.render.engine = "BLENDER_EEVEE_NEXT"
    scene.render.resolution_x = 1280
    scene.render.resolution_y = 800
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.view_settings.view_transform = "Standard"
    scene.view_settings.look = "None"

    world = bpy.data.worlds.new(f"DiagWorld_{scene.name}")
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes.get("Background")
    if bg:
        # Even neutral lighting so zero defects are masked in dark shadows
        bg.inputs["Color"].default_value = (0.24, 0.27, 0.31, 1.0)
        bg.inputs["Strength"].default_value = 1.15

    stage_col = bpy.data.collections.new(f"Stage_{scene.name}")
    scene.collection.children.link(stage_col)

    bm = bmesh.new()
    bmesh.ops.create_grid(bm, x_segments=1, y_segments=1, size=9.0)
    g_mesh = bpy.data.meshes.new(f"Ground_{scene.name}")
    bm.to_mesh(g_mesh)
    bm.free()
    g_mat = make_simple_material("GroundPlane", (0.26, 0.28, 0.32, 1.0), roughness=0.92)
    g_mesh.materials.append(g_mat)
    g_obj = bpy.data.objects.new(f"Ground_{scene.name}", g_mesh)
    g_obj.location = (0.0, 0.3, -0.002)
    stage_col.objects.link(g_obj)

    for lname, loc, rot_deg, energy, size in (
        ("KeyLight", (5.5, -5.5, 6.5), (48, 0, 45), 1100.0, 5.0),
        ("FillLight", (-6.0, -3.5, 3.8), (62, 0, -58), 850.0, 6.0),
        ("UnderFill", (0.0, -2.0, -3.0), (140, 0, 0), 400.0, 6.0),
        ("RimLight", (-2.0, 6.5, 5.0), (-50, 0, 165), 750.0, 4.5),
    ):
        ldata = bpy.data.lights.new(name=f"{lname}_{scene.name}", type="AREA")
        ldata.energy = energy
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


def render_view(scene, cam_obj, eye, target, out_path, ortho=False, ortho_scale=8.4, lens=36.0):
    bpy.context.window.scene = scene
    cam_obj.location = Vector(eye)
    cam_obj.rotation_euler = (Vector(target) - Vector(eye)).to_track_quat("-Z", "Y").to_euler()
    if ortho:
        cam_obj.data.type = "ORTHO"
        cam_obj.data.ortho_scale = ortho_scale
    else:
        cam_obj.data.type = "PERSP"
        cam_obj.data.lens = lens
    scene.render.filepath = str(out_path)
    t0 = time.perf_counter()
    bpy.ops.render.render(write_still=True)
    return round(time.perf_counter() - t0, 3)


def inspect_glb_binary(path):
    """Parse GLB 2.0 binary and verify schema, skins, weights, materials, and axes."""
    data = path.read_bytes()
    magic, version, total_len = struct.unpack_from("<4sII", data, 0)
    if magic != b"glTF" or version != 2 or total_len != len(data):
        raise ValueError("Invalid GLB 2.0 header")
    json_len, json_type = struct.unpack_from("<I4s", data, 12)
    document = json.loads(data[20:20 + json_len])
    bin_start = 20 + json_len
    bin_len, bin_type = struct.unpack_from("<I4s", data, bin_start)
    if bin_type != b"BIN\x00":
        raise ValueError("Missing BIN chunk")
    blob = data[bin_start + 8:bin_start + 8 + document["buffers"][0]["byteLength"]]

    fmt_map = {5121: "B", 5123: "H", 5125: "I", 5126: "f"}
    width_map = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}

    def read_acc(idx):
        acc = document["accessors"][idx]
        bv = document["bufferViews"][acc["bufferView"]]
        w = width_map[acc["type"]]
        fmt = "<" + fmt_map[acc["componentType"]] * w
        sz = struct.calcsize(fmt)
        base = bv.get("byteOffset", 0) + acc.get("byteOffset", 0)
        stride = bv.get("byteStride", sz)
        rows = [struct.unpack_from(fmt, blob, base + i * stride) for i in range(acc["count"])]
        if acc.get("normalized") and acc["componentType"] in (5121, 5123):
            denom = 255.0 if acc["componentType"] == 5121 else 65535.0
            rows = [tuple(val / denom for val in row) for row in rows]
        return rows

    nodes = document["nodes"]
    scenes = document["scenes"]
    default_scene = scenes[document["scene"]]
    if default_scene.get("name") != "Wyvern":
        raise ValueError(f"Expected default scene 'Wyvern', got {default_scene.get('name')!r}")

    # Collect all node names in default scene
    scene_node_indices = set()
    stack = list(default_scene["nodes"])
    while stack:
        ni = stack.pop()
        scene_node_indices.add(ni)
        stack.extend(nodes[ni].get("children", []))
    scene_node_names = {nodes[ni].get("name") for ni in scene_node_indices}

    # Ensure presentation / human helpers are NOT in GLB
    forbidden = [n for n in scene_node_names if n and ("Human" in n or "Ground" in n or "Light" in n or "Cam" in n)]
    if forbidden:
        raise ValueError(f"Helper objects leaked into GLB: {forbidden}")

    materials = {m["name"]: m for m in document.get("materials", [])}
    if set(materials.keys()) != {"Scales", "Horns", "Eyes", "Membranes"}:
        raise ValueError(f"Unexpected materials in GLB: {sorted(materials.keys())}")
    if not materials["Membranes"].get("doubleSided"):
        raise ValueError("Membranes material must have doubleSided=true")

    skins = document.get("skins", [])
    if len(skins) != 1:
        raise ValueError(f"Expected 1 skin, found {len(skins)}")
    skin = skins[0]
    joints = skin["joints"]
    ibms = read_acc(skin["inverseBindMatrices"])

    # Compute glTF node world transforms in bind pose
    children_map = {i: nodes[i].get("children", []) for i in range(len(nodes))}
    parent_map = {}
    for p, chs in children_map.items():
        for c in chs:
            parent_map[c] = p

    def node_local_mat(idx):
        n = nodes[idx]
        if "matrix" in n:
            m = n["matrix"]
            return Matrix((m[0:4], m[4:8], m[8:12], m[12:16])).transposed()
        t = Vector(n.get("translation", (0.0, 0.0, 0.0)))
        qx, qy, qz, qw = n.get("rotation", (0.0, 0.0, 0.0, 1.0))
        q = Quaternion((qw, qx, qy, qz))
        s = Vector(n.get("scale", (1.0, 1.0, 1.0)))
        return Matrix.LocRotScale(t, q, s)

    world_mats = {}
    for ni in scene_node_indices:
        chain = []
        cur = ni
        while cur is not None:
            chain.append(cur)
            cur = parent_map.get(cur)
        m = Matrix.Identity(4)
        for c in reversed(chain):
            m = m @ node_local_mat(c)
        world_mats[ni] = m

    ibm_errors = 0
    max_ibm_err = 0.0
    max_bind_rot_err = 0.0
    for j_idx, ibm_flat in zip(joints, ibms):
        m_ibm = Matrix((ibm_flat[0:4], ibm_flat[4:8], ibm_flat[8:12], ibm_flat[12:16])).transposed()
        prod = m_ibm @ world_mats[j_idx]
        err = max(abs(prod[r][c] - (1.0 if r == c else 0.0)) for r in range(4) for c in range(4))
        max_ibm_err = max(max_ibm_err, err)
        if err > 1e-4:
            ibm_errors += 1
        rot = nodes[j_idx].get("rotation", (0.0, 0.0, 0.0, 1.0))
        rot_err = max(abs(rot[0]), abs(rot[1]), abs(rot[2]), abs(rot[3] - 1.0))
        max_bind_rot_err = max(max_bind_rot_err, rot_err)

    mesh_nodes = [nodes[ni] for ni in scene_node_indices if "mesh" in nodes[ni]]
    if len(mesh_nodes) != 1:
        raise ValueError(f"Expected 1 mesh node, found {len(mesh_nodes)}")
    mesh_def = document["meshes"][mesh_nodes[0]["mesh"]]

    total_verts = 0
    total_tris = 0
    max_inf = 0
    norm_errors = 0
    unassigned = 0
    all_pos = []
    used_attrs = set()

    for prim in mesh_def["primitives"]:
        attrs = prim["attributes"]
        used_attrs.update(attrs.keys())
        pos = read_acc(attrs["POSITION"])
        jnt = read_acc(attrs["JOINTS_0"])
        wgt = read_acc(attrs["WEIGHTS_0"])
        idx = read_acc(prim["indices"])
        total_verts += len(pos)
        total_tris += len(idx) // 3
        all_pos.extend(pos)
        for j_row, w_row in zip(jnt, wgt):
            active = [(j, w) for j, w in zip(j_row, w_row) if w > 1e-6]
            if not active:
                unassigned += 1
            else:
                max_inf = max(max_inf, len(active))
                if abs(sum(w_row) - 1.0) > 1e-3:
                    norm_errors += 1

    bounds = [(min(axis), max(axis)) for axis in zip(*all_pos)]
    return {
        "file_size_bytes": len(data),
        "default_scene": default_scene.get("name"),
        "skinned_meshes": len(mesh_nodes),
        "primitives": len(mesh_def["primitives"]),
        "materials": sorted(materials.keys()),
        "membranes_double_sided": bool(materials["Membranes"].get("doubleSided")),
        "joints_count": len(joints),
        "joint_names": [nodes[j]["name"] for j in joints],
        "attributes": sorted(used_attrs),
        "vertices_exported": total_verts,
        "triangles_exported": total_tris,
        "max_influences": max_inf,
        "unassigned_vertices": unassigned,
        "weight_norm_errors": norm_errors,
        "ibm_errors": ibm_errors,
        "max_ibm_matrix_error": round(max_ibm_err, 7),
        "max_joint_bind_rotation_error": round(max_bind_rot_err, 7),
        "gltf_bounds_x_width_m": round(bounds[0][1] - bounds[0][0], 4),
        "gltf_bounds_y_height_m": round(bounds[1][1] - bounds[1][0], 4),
        "gltf_bounds_z_length_m": round(bounds[2][1] - bounds[2][0], 4),
        "gltf_min_m": [round(b[0], 4) for b in bounds],
        "gltf_max_m": [round(b[1], 4) for b in bounds],
    }


def roundtrip_reimport_check(glb_path, expected_bind_metrics, preview_out_path):
    """Re-import exported GLB into a fresh Blender scene and verify roundtrip fidelity."""
    sc_rt = bpy.data.scenes.new("Roundtrip_Verify")
    bpy.context.window.scene = sc_rt
    sc_rt.unit_settings.system = "METRIC"
    sc_rt.unit_settings.scale_length = 1.0

    before_objs = set(bpy.data.objects)
    res = bpy.ops.import_scene.gltf(filepath=str(glb_path))
    if res != {"FINISHED"}:
        raise RuntimeError(f"glTF roundtrip import failed: {res}")
    imported_objs = [o for o in bpy.data.objects if o not in before_objs]
    scene_imported = [
        o for o in imported_objs
        if "glTF_not_exported" not in [c.name for c in o.users_collection]
    ]

    imp_arms = [o for o in scene_imported if o.type == "ARMATURE"]
    imp_meshes = [o for o in scene_imported if o.type == "MESH"]
    if len(imp_arms) != 1 or len(imp_meshes) != 1:
        raise RuntimeError(f"Unexpected imported object counts: arms={len(imp_arms)}, meshes={len(imp_meshes)}")

    imp_arm = imp_arms[0]
    imp_mesh = imp_meshes[0]
    # Note: Blender glTF importer splits sharp/vertex-colored attributes unless merged; compare vertex/tri & world AABB
    depsgraph = bpy.context.evaluated_depsgraph_get()
    eval_obj = imp_mesh.evaluated_get(depsgraph)
    me = eval_obj.to_mesh()
    me.calc_loop_triangles()
    min_pt = Vector((float("inf"), float("inf"), float("inf")))
    max_pt = Vector((float("-inf"), float("-inf"), float("-inf")))
    for v in me.vertices:
        wp = imp_mesh.matrix_world @ v.co
        min_pt.x = min(min_pt.x, wp.x)
        min_pt.y = min(min_pt.y, wp.y)
        min_pt.z = min(min_pt.z, wp.z)
        max_pt.x = max(max_pt.x, wp.x)
        max_pt.y = max(max_pt.y, wp.y)
        max_pt.z = max(max_pt.z, wp.z)
    rt_tris = len(me.loop_triangles)
    rt_verts = len(me.vertices)
    eval_obj.to_mesh_clear()

    dims = max_pt - min_pt
    dx_err = abs(round(dims.x, 4) - expected_bind_metrics["width_x_m"])
    dy_err = abs(round(dims.y, 4) - expected_bind_metrics["length_y_m"])
    dz_err = abs(round(dims.z, 4) - expected_bind_metrics["height_z_m"])

    # Verify asymmetric orientation after roundtrip: head/snout at -Y, tail at +Y, chipped horn at -X
    head_bone = imp_arm.data.bones.get("Head")
    head_world = imp_arm.matrix_world @ head_bone.head_local if head_bone else Vector((0, 0, 0))

    cam_rt = setup_diagnostic_stage(sc_rt)
    render_time = render_view(
        sc_rt, cam_rt, (6.2, -6.8, 3.6), (0.0, 0.25, 1.25),
        preview_out_path, ortho=False, lens=36.0
    )

    result = {
        "imported_armature_bones": len(imp_arm.data.bones),
        "imported_mesh_materials": [m.name for m in imp_mesh.data.materials],
        "imported_vertices": rt_verts,
        "imported_triangles": rt_tris,
        "roundtrip_width_x_m": round(dims.x, 4),
        "roundtrip_length_y_m": round(dims.y, 4),
        "roundtrip_height_z_m": round(dims.z, 4),
        "roundtrip_bounds_min_m": [round(min_pt.x, 4), round(min_pt.y, 4), round(min_pt.z, 4)],
        "roundtrip_bounds_max_m": [round(max_pt.x, 4), round(max_pt.y, 4), round(max_pt.z, 4)],
        "dimension_deltas_m": [round(dx_err, 5), round(dy_err, 5), round(dz_err, 5)],
        "head_world_after_reimport": [round(head_world.x, 4), round(head_world.y, 4), round(head_world.z, 4)],
        "render_time_sec": render_time,
    }

    # Clean temporary roundtrip scene before saving final wyvern.blend
    bpy.data.scenes.remove(sc_rt)
    for o in imported_objs:
        if o.name in bpy.data.objects:
            bpy.data.objects.remove(o, do_unlink=True)

    return result


def write_art_manifest(path):
    """Prepare assets/creatures/wyvern_art.json matching wyvern.json contract (not yet ready for integration)."""
    manifest = {
        "schema_version": 1,
        "model": "creatures/wyvern_art/wyvern.glb",
        "scene": "Wyvern",
        "source_forward": "+Z",
        "up_axis": "+Y",
        "units": "meters",
        "scale": 1.0,
        "import_correction": "bevy_gltf_scene_rotation",
        "pivot": "ground",
        "root_motion": "in_place",
        "clips": {
            "IdleGround": "IdleGround",
            "Walk": "Walk",
            "Takeoff": "Takeoff",
            "Flap": "Flap",
            "Glide": "Glide",
            "Land": "Land",
        },
        "slots": {
            "Scales": "Scales",
            "Horns": "Horns",
            "Eyes": "Eyes",
            "Membranes": "Membranes",
        },
        "palettes": {
            "Jade": {
                "Scales": [0.10, 0.38, 0.29],
                "Horns": [0.66, 0.58, 0.39],
                "Eyes": [1.0, 0.72, 0.20],
                "Membranes": [0.18, 0.63, 0.55],
            },
            "Braise": {
                "Scales": [0.51, 0.15, 0.08],
                "Horns": [0.25, 0.16, 0.12],
                "Eyes": [1.0, 0.80, 0.27],
                "Membranes": [0.76, 0.32, 0.11],
            },
            "Obsidienne": {
                "Scales": [0.13, 0.17, 0.23],
                "Horns": [0.56, 0.49, 0.60],
                "Eyes": [0.54, 0.83, 1.0],
                "Membranes": [0.16, 0.48, 0.62],
            },
        },
    }
    path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--blend", type=Path, default=DEFAULT_BLEND)
    parser.add_argument("--glb", type=Path, default=DEFAULT_GLB)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--previews", type=Path, default=DEFAULT_PREVIEW_DIR)
    parser.add_argument("--metrics", type=Path, default=DEFAULT_METRICS)
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else [])

    bpy.ops.wm.read_factory_settings(use_empty=True)

    mat_scales = make_material("Scales", PALETTE_JADE["Scales"], roughness=0.76)
    mat_horns = make_material("Horns", PALETTE_JADE["Horns"], roughness=0.62)
    mat_eyes = make_material("Eyes", PALETTE_JADE["Eyes"], roughness=0.28, emission=0.8)
    mat_mem = make_material("Membranes", PALETTE_JADE["Membranes"], roughness=0.84, double_sided=True)
    mats = [mat_scales, mat_horns, mat_eyes, mat_mem]

    # 1. Clean exportable Scene `Wyvern`
    scene_wyvern = bpy.context.scene
    scene_wyvern.name = "Wyvern"
    scene_wyvern.unit_settings.system = "METRIC"
    scene_wyvern.unit_settings.scale_length = 1.0
    col_wyvern = bpy.data.collections.new("Wyvern_Rigged")
    scene_wyvern.collection.children.link(col_wyvern)

    pts, hierarchy = define_skeleton_points()
    arm_obj = create_armature(scene_wyvern, col_wyvern, pts, hierarchy)
    mesh_obj = build_unified_skinned_mesh(col_wyvern, arm_obj, pts, hierarchy, mats)
    bpy.context.view_layer.update()

    weight_audit = audit_mesh_weights(mesh_obj, hierarchy)

    # 2. Diagnostic Presentation scene (links same rigged wyvern + 1.80 m human)
    scene_pres = bpy.data.scenes.new("Presentation")
    bpy.context.window.scene = scene_pres
    cam_pres = setup_diagnostic_stage(scene_pres)
    scene_pres.collection.children.link(col_wyvern)
    col_human = bpy.data.collections.new("ScaleReference_Human_180cm")
    scene_pres.collection.children.link(col_human)
    create_human_reference(col_human, location=(2.20, -2.35, 0.0))
    bpy.context.view_layer.update()

    # 3. Test all 7 deformation poses on the EXACT same topology & armature
    cam_front = ((0.0, -9.5, 1.45), (0.15, 0.0, 1.35), True, 8.4, 36.0)
    cam_profile = ((-9.5, 0.45, 1.45), (0.0, 0.45, 1.35), True, 7.6, 36.0)
    cam_top = ((0.0, 0.50, 11.5), (0.0, 0.50, 1.2), True, 10.6, 36.0)
    cam_3q = ((6.2, -6.8, 3.6), (0.2, 0.25, 1.25), False, 8.4, 36.0)

    args.previews.mkdir(parents=True, exist_ok=True)
    pose_metrics = {}
    render_times = {}

    # Pose 1: Bind/Open
    apply_test_pose(arm_obj, "BindOpen", pts)
    m_bind, coords_bind = measure_deformed_mesh(mesh_obj)
    pose_metrics["BindOpen"] = m_bind
    for fname, cfg in (
        ("c1_open_front.png", cam_front),
        ("c1_open_profile.png", cam_profile),
        ("c1_open_top.png", cam_top),
        ("c1_open_three_quarter.png", cam_3q),
    ):
        render_times[fname] = render_view(
            scene_pres, cam_pres, cfg[0], cfg[1], args.previews / fname,
            ortho=cfg[2], ortho_scale=cfg[3], lens=cfg[4]
        )

    # Pose 2: FoldedWings
    apply_test_pose(arm_obj, "FoldedWings", pts)
    m_folded, _ = measure_deformed_mesh(mesh_obj)
    pose_metrics["FoldedWings"] = m_folded
    for fname, cfg in (
        ("c1_folded_front.png", cam_front),
        ("c1_folded_profile.png", cam_profile),
        ("c1_folded_three_quarter.png", cam_3q),
    ):
        render_times[fname] = render_view(
            scene_pres, cam_pres, cfg[0], cfg[1], args.previews / fname,
            ortho=cfg[2], ortho_scale=cfg[3], lens=cfg[4]
        )

    # Poses 3-6: Individual pose metrics + Combined articulation renders
    for p_name in ("NeckHeadFlex", "JawOpen", "LegsFlex", "TailCurl"):
        apply_test_pose(arm_obj, p_name, pts)
        pm, _ = measure_deformed_mesh(mesh_obj)
        pose_metrics[p_name] = pm

    apply_test_pose(arm_obj, "ArticulationCombined", pts)
    m_comb, _ = measure_deformed_mesh(mesh_obj)
    pose_metrics["ArticulationCombined"] = m_comb
    for fname, cfg in (
        ("c1_articulation_profile.png", cam_profile),
        ("c1_articulation_three_quarter.png", cam_3q),
    ):
        render_times[fname] = render_view(
            scene_pres, cam_pres, cfg[0], cfg[1], args.previews / fname,
            ortho=cfg[2], ortho_scale=cfg[3], lens=cfg[4]
        )

    # Pose 7: Return to Bind Pose and check exact vertex restoration
    reset_pose(arm_obj)
    m_return, coords_return = measure_deformed_mesh(mesh_obj)
    max_return_drift = max(
        math.dist(a, b) for a, b in zip(coords_bind, coords_return)
    )
    pose_metrics["BindReturn"] = {
        **m_return,
        "max_vertex_drift_m": round(max_return_drift, 8),
    }

    # 4. Technical GLB export from clean `Wyvern` scene (rest pose, no helpers)
    bpy.context.window.scene = scene_wyvern
    args.glb.parent.mkdir(parents=True, exist_ok=True)
    res_exp = bpy.ops.export_scene.gltf(
        filepath=str(args.glb),
        export_format="GLB",
        use_active_scene=True,
        export_yup=True,
        export_cameras=False,
        export_lights=False,
        export_vertex_color="ACTIVE",
        export_tangents=True,
        export_skins=True,
        export_influence_nb=4,
        export_all_influences=False,
        export_animations=False,
        export_rest_position_armature=True,
    )
    if res_exp != {"FINISHED"}:
        raise RuntimeError(f"GLB export failed: {res_exp}")

    glb_audit = inspect_glb_binary(args.glb)
    roundtrip = roundtrip_reimport_check(args.glb, m_bind, args.previews / "c1_reimport_roundtrip.png")
    render_times["c1_reimport_roundtrip.png"] = roundtrip["render_time_sec"]

    # Save compressed .blend in exact rest pose
    bpy.context.window.scene = scene_wyvern
    reset_pose(arm_obj)
    args.blend.parent.mkdir(parents=True, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=str(args.blend), compress=True)

    # Prepare wyvern_art.json manifest (status in QUEUE.json stays IN_PROGRESS until ART-C2 animations)
    write_art_manifest(args.manifest)

    report = {
        "pass": "ART-C1",
        "blender_version": bpy.app.version_string,
        "blend_path": str(args.blend),
        "blend_size_bytes": args.blend.stat().st_size,
        "glb_path": str(args.glb),
        "manifest_path": str(args.manifest),
        "armature_bones_count": len(arm_obj.data.bones),
        "armature_bones": [name for name, _ in hierarchy],
        "skinned_mesh_objects": 1,
        "materials_count": len(mats),
        "materials": [m.name for m in mats],
        "weight_audit": weight_audit,
        "pose_metrics": pose_metrics,
        "glb_audit": glb_audit,
        "roundtrip_reimport": roundtrip,
        "render_times_sec": render_times,
        "total_render_time_sec": round(sum(render_times.values()), 3),
    }
    args.metrics.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
