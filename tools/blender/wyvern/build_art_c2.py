"""ART-C2: Definitive rigged, textured, and animated Primal Voxel wyvern (.blend + previews).

Builds on ART-C1 unified topology (`Wyvern_Mesh`, 23-bone `Wyvern_Rig`, 4 materials) with:
- Phase 1 fixes: sealed outer wingtip membrane anchor, clean torso shoulder weights,
  clean folded wrist/elbow alignment, and exact foot-ground contact (`Z_min ~ 0.000 m`)
- Phase 4 PNG textures under assets/creatures/wyvern_art/textures/ (relative //textures/...)
  using neutral luminance masks + UVs so Jade, Braise, and Obsidienne tint cleanly in Bevy
- Phase 2 six contractual Actions at 100 fps (`IdleGround` 3.20s, `Walk` 1.04s, `Takeoff` 1.05s,
  `Flap` 1.02s, `Glide` 3.20s, `Land` 1.10s) with C1 loop closure and zero horizontal root motion
- Phase 3 transition & crossfade checks (220 ms Bevy blend simulation) + diagnostic renders
"""

import argparse
import json
import math
import sys
import time
from collections import defaultdict
from pathlib import Path

import bmesh
import bpy
from mathutils import Euler, Matrix, Quaternion, Vector

sys.path.insert(0, str(Path(__file__).resolve().parent))
from generate_textures import generate_all as generate_png_textures


REPO_ROOT = Path(__file__).resolve().parents[3]
ASSET_DIR = REPO_ROOT / "assets/creatures/wyvern_art"
TEX_DIR = ASSET_DIR / "textures"
DEFAULT_BLEND = ASSET_DIR / "wyvern.blend"
DEFAULT_MANIFEST = REPO_ROOT / "assets/creatures/wyvern_art.json"
DEFAULT_PREVIEW_DIR = REPO_ROOT / "art_pipeline/previews/wyvern/art_c2"
DEFAULT_METRICS = DEFAULT_PREVIEW_DIR / "art_c2_metrics.json"

PALETTES = {
    "Jade": {
        "Scales": [0.11, 0.42, 0.32],
        "Horns": [0.76, 0.67, 0.45],
        "Eyes": [1.0, 0.72, 0.2],
        "Membranes": [0.18, 0.65, 0.56],
    },
    "Braise": {
        "Scales": [0.55, 0.14, 0.08],
        "Horns": [0.34, 0.24, 0.18],
        "Eyes": [1.0, 0.8, 0.27],
        "Membranes": [0.92, 0.42, 0.15],
    },
    "Obsidienne": {
        "Scales": [0.13, 0.17, 0.23],
        "Horns": [0.56, 0.49, 0.6],
        "Eyes": [0.54, 0.83, 1.0],
        "Membranes": [0.16, 0.48, 0.62],
    },
}

# Neutral luminance bands [0..1] in COLOR_0 so all 3 Bevy palettes tint cleanly without saturation crushing
SHADE_DORSAL = (0.64, 0.68, 0.66)
SHADE_MID = (0.86, 0.88, 0.86)
SHADE_FLANK = (0.78, 0.80, 0.78)
SHADE_BELLY = (1.00, 0.94, 0.84)
SHADE_DARK = (0.54, 0.56, 0.54)
SHADE_EYE_SOCKET = (0.20, 0.20, 0.20)
SHADE_HORN_BASE = (0.82, 0.80, 0.76)
SHADE_HORN_TIP = (1.00, 0.98, 0.95)


def srgb_to_linear(c):
    if c <= 0.04045:
        return c / 12.92
    return ((c + 0.055) / 1.055) ** 2.4


def load_relative_image(filename, is_non_color=False):
    abs_path = (TEX_DIR / filename).resolve()
    img = bpy.data.images.load(str(abs_path), check_existing=True)
    img.filepath = f"//textures/{filename}"
    img.filepath_raw = f"//textures/{filename}"
    if is_non_color:
        img.colorspace_settings.name = "Non-Color"
    else:
        img.colorspace_settings.name = "sRGB"
    return img


def make_textured_material(name, albedo_png, normal_png=None, roughness=0.74, double_sided=False):
    mat = bpy.data.materials.get(name)
    if mat is None:
        mat = bpy.data.materials.new(name=name)
    mat.use_nodes = True
    mat.use_backface_culling = not double_sided
    nt = mat.node_tree
    nodes = nt.nodes
    links = nt.links
    nodes.clear()

    out = nodes.new("ShaderNodeOutputMaterial")
    out.location = (480, 0)

    bsdf = nodes.new("ShaderNodeBsdfPrincipled")
    bsdf.location = (180, 0)
    bsdf.inputs["Roughness"].default_value = roughness
    if "Specular IOR Level" in bsdf.inputs:
        bsdf.inputs["Specular IOR Level"].default_value = 0.26

    tex_albedo = nodes.new("ShaderNodeTexImage")
    tex_albedo.image = load_relative_image(albedo_png, is_non_color=False)
    tex_albedo.location = (-420, 180)

    vcol = nodes.new("ShaderNodeVertexColor")
    vcol.layer_name = "Col"
    vcol.location = (-420, -40)

    mul = nodes.new("ShaderNodeMix")
    mul.data_type = "RGBA"
    mul.blend_type = "MULTIPLY"
    mul.inputs["Factor"].default_value = 1.0
    mul.location = (-140, 100)

    links.new(tex_albedo.outputs["Color"], mul.inputs["A"])
    links.new(vcol.outputs["Color"], mul.inputs["B"])
    links.new(mul.outputs["Result"], bsdf.inputs["Base Color"])

    if normal_png is not None:
        tex_norm = nodes.new("ShaderNodeTexImage")
        tex_norm.image = load_relative_image(normal_png, is_non_color=True)
        tex_norm.location = (-420, -280)
        nmap = nodes.new("ShaderNodeNormalMap")
        nmap.inputs["Strength"].default_value = 0.65
        nmap.location = (-120, -240)
        links.new(tex_norm.outputs["Color"], nmap.inputs["Color"])
        links.new(nmap.outputs["Normal"], bsdf.inputs["Normal"])

    links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
    return mat


def set_preview_palette(mats, palette_name):
    """Temporarily multiply preview palette tint in Blender shader for Eevee inspection renders."""
    pal = PALETTES[palette_name]
    for mat in mats:
        nt = mat.node_tree
        bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
        pal_node = nt.nodes.get("PalettePreviewTint")
        rgb = pal[mat.name]
        lin = tuple(srgb_to_linear(v) for v in rgb) + (1.0,)
        if pal_node is None:
            # Rewire existing Base Color source through PalettePreviewTint
            src_link = bsdf.inputs["Base Color"].links[0]
            src_socket = src_link.from_socket
            nt.links.remove(src_link)
            pal_node = nt.nodes.new("ShaderNodeMix")
            pal_node.name = "PalettePreviewTint"
            pal_node.data_type = "RGBA"
            pal_node.blend_type = "MULTIPLY"
            pal_node.inputs["Factor"].default_value = 1.0
            pal_node.location = (20, 120)
            nt.links.new(src_socket, pal_node.inputs["A"])
            nt.links.new(pal_node.outputs["Result"], bsdf.inputs["Base Color"])
        pal_node.inputs["B"].default_value = lin


def clear_preview_palette(mats):
    """Restore pure neutral shader wiring (baseColorFactor=[1,1,1,1] in glTF) for Bevy."""
    for mat in mats:
        nt = mat.node_tree
        pal_node = nt.nodes.get("PalettePreviewTint")
        if pal_node is not None:
            bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
            src_socket = pal_node.inputs["A"].links[0].from_socket
            nt.links.remove(bsdf.inputs["Base Color"].links[0])
            nt.links.remove(pal_node.inputs["A"].links[0])
            nt.nodes.remove(pal_node)
            nt.links.new(src_socket, bsdf.inputs["Base Color"])


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
    cleaned = [(bone, float(w)) for bone, w in weight_dict.items() if w > 1e-6]
    cleaned.sort(key=lambda item: (-item[1], item[0]))
    cleaned = cleaned[:max_influences]
    if not cleaned:
        return {"Body": 1.0}
    total = sum(w for _, w in cleaned)
    if total <= 1e-8:
        return {"Body": 1.0}
    normed = {bone: w / total for bone, w in cleaned}
    max_bone = cleaned[0][0]
    diff = 1.0 - sum(normed.values())
    normed[max_bone] = max(0.0, min(1.0, normed[max_bone] + diff))
    return normed


def blend_weights(pairs):
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
    """Sparse 3D voxel grid with interior culling, band-aware greedy merging, and crisp tile UVs."""

    def __init__(self, step=0.06, min_z_world=None):
        self.step = step
        self.min_z_world = min_z_world
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

    def emit_into_bmesh(self, bm, color_layer, uv_layer, weight_fn, vert_weights_out, eyes_mat_idx=2):
        s = self.step
        dirs = [
            (0, +1, 1, 2, False),
            (0, -1, 1, 2, True),
            (1, +1, 0, 2, True),
            (1, -1, 0, 2, False),
            (2, +1, 0, 1, False),
            (2, -1, 0, 1, True),
        ]

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
                    mat_idx, qrgb, _ = token
                    if mat_idx == eyes_mat_idx:
                        # Map eye block faces cleanly to [0..1]x[0..1] for centered slit pupil
                        tex_uvs = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]
                    else:
                        # 16 voxel cells per 1024x1024 tile -> crisp scale texels aligned to integer voxel grid
                        tu0, tv0 = (u0 % 16) / 16.0, (v0 % 16) / 16.0
                        tex_uvs = [
                            (tu0, tv0),
                            (tu0 + w / 16.0, tv0),
                            (tu0 + w / 16.0, tv0 + h / 16.0),
                            (tu0, tv0 + h / 16.0),
                        ]

                    if flip:
                        corners_uv = [corners_uv[0], corners_uv[3], corners_uv[2], corners_uv[1]]
                        tex_uvs = [tex_uvs[0], tex_uvs[3], tex_uvs[2], tex_uvs[1]]

                    verts = []
                    for cu, cv in corners_uv:
                        p = [0.0, 0.0, 0.0]
                        p[axis] = plane_coord
                        p[u_ax] = cu
                        p[v_ax] = cv
                        verts.append(get_vert(tuple(p)))

                    rgba = (qrgb[0], qrgb[1], qrgb[2], 1.0)
                    for idx_tri in ((0, 1, 2), (0, 2, 3)):
                        tri_verts = (verts[idx_tri[0]], verts[idx_tri[1]], verts[idx_tri[2]])
                        tri_uvs = (tex_uvs[idx_tri[0]], tex_uvs[idx_tri[1]], tex_uvs[idx_tri[2]])
                        try:
                            face = bm.faces.new(tri_verts)
                        except ValueError:
                            continue
                        face.material_index = mat_idx
                        face.smooth = False
                        for loop, uv in zip(face.loops, tri_uvs):
                            loop[color_layer] = rgba
                            loop[uv_layer].uv = uv


def emit_unified_voxel_membrane(bm, color_layer, uv_layer, mat_idx_mem, anchors_w, bays, vert_weights_out, voxel_step=0.055):
    rows = 5
    cols = 6

    def qstep(val, step):
        return round(val / step) * step

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
        # Internal bays (scallop == 0) avoid the dark trailing-edge hem at V=1.0
        v_max_uv = 1.0 if scallop > 0.0 else 0.84

        grid = {}
        for r in range(rows + 1):
            t_r = r / rows
            for c in range(cols + 1):
                t_c = c / cols
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
                    if not (c == 1 and key_p == "wd" and key_a == "tip"):
                        pt.x = qstep(pt.x, voxel_step)
                        pt.y = qstep(pt.y, voxel_step)
                        pt.z = qstep(pt.z, voxel_step * 0.8)

                if 0 < r < rows:
                    camber = (0.042 if r in (2, 3) else 0.022) * (1.0 - abs(2.0 * t_c - 1.0) * 0.5)
                    pt = pt + Vector((0.0, 0.0, camber))

                grid[(r, c)] = make_vert(pt, w_dict, shared_key=skey)

        for r in range(rows):
            v_uv0 = (r / rows) * v_max_uv
            v_uv1 = ((r + 1) / rows) * v_max_uv
            for c in range(cols):
                u_uv0 = c / cols
                u_uv1 = (c + 1) / cols
                v00 = grid[(r, c)]
                v10 = grid[(r + 1, c)]
                v11 = grid[(r + 1, c + 1)]
                v01 = grid[(r, c + 1)]
                uv00 = (u_uv0, v_uv0)
                uv10 = (u_uv0, v_uv1)
                uv11 = (u_uv1, v_uv1)
                uv01 = (u_uv1, v_uv0)

                rib_shade = 0.78 if c in (0, cols - 1) else (0.98 if c in (2, 3) else 0.88)
                row_grad = 0.86 + 0.14 * (r / rows)
                if scallop > 0.0 and r == rows - 1:
                    row_grad *= 0.84
                s_val = max(0.62, min(1.0, rib_shade * row_grad))
                col = (s_val, s_val, s_val, 1.0)

                for tri_verts, tri_uvs in (
                    ((v00, v10, v11), (uv00, uv10, uv11)),
                    ((v00, v11, v01), (uv00, uv11, uv01)),
                ):
                    if len({tri_verts[0], tri_verts[1], tri_verts[2]}) < 3:
                        continue
                    try:
                        f = bm.faces.new(tri_verts)
                        f.material_index = mat_idx_mem
                        f.smooth = False
                        for loop, uv in zip(f.loops, tri_uvs):
                            loop[color_layer] = col
                            loop[uv_layer].uv = uv
                    except ValueError:
                        pass


def define_skeleton_points():
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
    """Build ONE unified voxel + membrane mesh with UVs, neutral vertex colors, and normalized skinning."""
    SCALES, HORNS, EYES, MEMBRANES = 0, 1, 2, 3
    bm = bmesh.new()
    color_layer = bm.loops.layers.color.new("Col")
    uv_layer = bm.loops.layers.uv.new("UVMap")
    vert_weights = {}

    def belly_shade(wx, wy, wz, t, nz):
        if nz < -0.32:
            return SHADE_BELLY
        if nz > 0.56:
            return SHADE_DORSAL
        return SHADE_MID

    p_nb = Vector(pts["NeckBase"])
    p_nt = Vector(pts["NeckTip"])
    p_head = Vector(pts["Head"])
    p_tb = Vector(pts["TailBase"])
    p_tm = Vector(pts["TailMid"])
    p_tt = Vector(pts["TailTip"])

    # 1. CONTINUOUS SPINE: BODY + NECK + TAIL
    g_spine = VoxelGrid(step=0.062)

    def spine_band(wx, wy, wz):
        if wy < -0.35 or wy > 0.50:
            return int(round(wy / 0.16))
        return 0

    def spine_weight(pt):
        if pt.y < -0.32:
            # Keep pectoral chest center rigid on Body; only lateral shoulder socket blends with WingShoulder
            if pt.y >= p_nb.y:
                if abs(pt.x) > 0.34 and -0.56 < pt.y < -0.28 and pt.z > 1.32:
                    side = "L" if pt.x > 0 else "R"
                    d_ws = (pt - Vector(pts[f"WingShoulder.{side}"])).length
                    if d_ws < 0.28:
                        f = 0.28 * (1.0 - smoothstep(0.08, 0.28, d_ws))
                        return {"Body": 1.0 - f, f"WingShoulder.{side}": f}
                u = smoothstep(-0.32, p_nb.y, pt.y)
                return {"Body": 1.0 - 0.65 * u, "NeckBase": 0.65 * u}
            if pt.y >= p_nt.y:
                u = smoothstep(p_nb.y, p_nt.y, pt.y)
                return {"NeckBase": 1.0 - u, "NeckTip": u}
            u = smoothstep(p_nt.y, p_head.y, pt.y)
            return {"NeckTip": 1.0 - 0.85 * u, "Head": 0.85 * u}

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

        # Lateral hip & shoulder socket blends (chest keel remains 1.0 Body -> zero pectoral pull)
        w = {"Body": 1.0}
        if abs(pt.x) > 0.32:
            side = "L" if pt.x > 0 else "R"
            ws_p = Vector(pts[f"WingShoulder.{side}"])
            hp_p = Vector(pts[f"Hip.{side}"])
            d_ws = (pt - ws_p).length
            d_hp = (pt - hp_p).length
            if d_ws < 0.26 and pt.y > -0.54:
                f = 0.28 * (1.0 - smoothstep(0.08, 0.26, d_ws))
                w = {"Body": 1.0 - f, f"WingShoulder.{side}": f}
            elif d_hp < 0.30:
                f = 0.38 * (1.0 - smoothstep(0.08, 0.30, d_hp))
                w = {"Body": 1.0 - f, f"Hip.{side}": f}
        return w

    g_spine.add_world_capsule((0, 0.84, 1.20), (0, 0.30, 1.26), 0.30, 0.38, SCALES, belly_shade, spine_band, flatten_x=0.90, flatten_z=0.86, keel_power=0.38)
    g_spine.add_world_capsule((0, 0.30, 1.26), (0, -0.34, 1.38), 0.38, 0.50, SCALES, belly_shade, spine_band, flatten_x=0.92, flatten_z=0.96, keel_power=0.52)
    g_spine.add_world_capsule((0, -0.34, 1.38), (0, -0.68, 1.48), 0.48, 0.31, SCALES, belly_shade, spine_band, flatten_x=0.92, flatten_z=0.92, keel_power=0.45)
    g_spine.add_world_capsule((0, -0.54, 1.26), (0, 0.04, 1.06), 0.16, 0.24, SCALES, lambda *a: SHADE_BELLY, spine_band, flatten_x=0.58, flatten_z=1.10)
    for sign in (1.0, -1.0):
        g_spine.add_world_capsule((0.28 * sign, -0.40, 1.56), (0.36 * sign, -0.05, 1.62), 0.14, 0.11, SCALES, lambda *a: SHADE_DORSAL, spine_band)

    g_spine.add_world_capsule((0, -0.46, 1.42), (0, -1.06, 1.88), 0.31, 0.23, SCALES, belly_shade, spine_band, flatten_x=0.84, flatten_z=0.98, keel_power=0.35)
    g_spine.add_world_capsule((0, -1.02, 1.86), (0, -1.62, 2.22), 0.23, 0.20, SCALES, belly_shade, spine_band, flatten_x=0.82, flatten_z=0.96, keel_power=0.30)

    g_spine.add_world_capsule((0, 0.66, 1.21), (0, 1.70, 1.03), 0.29, 0.19, SCALES, belly_shade, spine_band, keel_power=0.25)
    g_spine.add_world_capsule((0.0, 1.66, 1.04), (0.06, 2.60, 0.80), 0.19, 0.11, SCALES, belly_shade, spine_band)
    g_spine.add_world_capsule((0.06, 2.56, 0.81), (0.01, 3.12, 0.84), 0.11, 0.065, SCALES, belly_shade, spine_band)
    g_spine.add_world_capsule((0.01, 3.12, 0.84), (-0.16, 3.63, 1.02), 0.065, 0.032, SCALES, belly_shade, spine_band)
    g_spine.add_world_ellipsoid((-0.18, 3.53, 1.02), (0.14, 0.16, 0.045), HORNS, lambda *a: SHADE_HORN_BASE, spine_band)

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

    g_spine.emit_into_bmesh(bm, color_layer, uv_layer, spine_weight, vert_weights, eyes_mat_idx=EYES)

    # 2. HEAD & ARTICULATED JAW
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
        bm, color_layer, uv_layer,
        lambda pt: {"Head": 1.0} if (pt.y < -1.56 or pt.z > 2.38) else {"Head": 0.85, "NeckTip": 0.15},
        vert_weights, eyes_mat_idx=EYES,
    )

    g_jaw = VoxelGrid(step=0.048)
    g_jaw.add_world_capsule((0, -1.64, 2.04), (0, -2.52, 2.07), 0.22, 0.088, SCALES, lambda *a: SHADE_BELLY, flatten_x=0.98, flatten_z=0.55)
    g_jaw.add_world_capsule((0, -1.78, 1.98), (0, -1.62, 1.88), 0.05, 0.022, HORNS, lambda *a: SHADE_HORN_BASE)
    for sign in (1.0, -1.0):
        g_jaw.add_world_box((0.10 * sign - 0.025, -2.42, 2.09), (0.10 * sign + 0.025, -2.35, 2.15), HORNS, SHADE_HORN_TIP)
    g_jaw.emit_into_bmesh(bm, color_layer, uv_layer, lambda pt: {"Jaw": 1.0}, vert_weights, eyes_mat_idx=EYES)

    # 3. CONTINUOUS HIND LEGS
    for side, sign in (("L", 1.0), ("R", -1.0)):
        h_pt = Vector(pts[f"Hip.{side}"])
        k_pt = Vector(pts[f"Knee.{side}"])
        a_pt = Vector(pts[f"Ankle.{side}"])
        f_pt = Vector((0.60 * sign, 0.54, 0.09))

        def leg_band(wx, wy, wz):
            return int(round(wz / 0.15))

        def leg_weight(pt, side=side, h_pt=h_pt, k_pt=k_pt, a_pt=a_pt):
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
        g_leg.emit_into_bmesh(bm, color_layer, uv_layer, leg_weight, vert_weights, eyes_mat_idx=EYES)

    # 4. CONTINUOUS WINGS + SEALED 7-BAY MEMBRANES
    for side, sign in (("L", 1.0), ("R", -1.0)):
        ws = Vector(pts[f"WingShoulder.{side}"])
        we = Vector(pts[f"WingElbow.{side}"])
        ww = Vector(pts[f"WingWrist.{side}"])
        wd = Vector(pts[f"WingDigit.{side}"])
        tip_outer = Vector((3.56 * sign, 0.36, 1.25))
        f2_tip = Vector((3.15 * sign, 1.25, 1.19))
        f3_tip = Vector((2.48 * sign, 1.68, 1.11))
        f4_tip = Vector((1.60 * sign, 1.64, 1.08))
        flank = Vector((0.42 * sign, 0.64, 1.15))

        w_ws = {f"WingShoulder.{side}": 0.82, "Body": 0.18}
        w_we = {f"WingElbow.{side}": 0.68, f"WingShoulder.{side}": 0.32}
        w_ww = {f"WingWrist.{side}": 1.0}
        w_wd = {f"WingDigit.{side}": 0.65, f"WingWrist.{side}": 0.35}
        w_tip = {f"WingDigit.{side}": 1.0}
        w_f2 = {f"WingDigit.{side}": 0.65, f"WingWrist.{side}": 0.35}
        w_f3 = {f"WingDigit.{side}": 0.25, f"WingWrist.{side}": 0.75}
        w_f4 = {f"WingWrist.{side}": 0.82, f"WingElbow.{side}": 0.18}
        w_flank = {"Body": 0.90, f"WingShoulder.{side}": 0.10}

        def wing_band(wx, wy, wz):
            return int(round(abs(wx) / 0.16)) * 100 + int(round(wy / 0.22))

        def wing_weight(
            pt, side=side, sign=sign, ws=ws, we=we, ww=ww, wd=wd,
            tip_outer=tip_outer, f2_tip=f2_tip, f3_tip=f3_tip, f4_tip=f4_tip,
            w_f2=w_f2, w_f3=w_f3, w_f4=w_f4
        ):
            t_se, d_se = segment_param(pt, ws, we)
            t_ew, d_ew = segment_param(pt, we, ww)
            t_wd, d_wd = segment_param(pt, ww, wd)
            t_dt, d_dt = segment_param(pt, wd, tip_outer)
            d_main = min(d_se, d_ew, d_wd, d_dt)

            t2, d2 = segment_param(pt, ww, f2_tip)
            t3, d3 = segment_param(pt, ww, f3_tip)
            t4, d4 = segment_param(pt, ww, f4_tip)
            best_t, best_df, best_w = min(
                (t2, d2, w_f2), (t3, d3, w_f3), (t4, d4, w_f4),
                key=lambda item: item[1],
            )
            if pt.y > ww.y + 0.12 and (pt - wd).length > 0.25 and best_df < d_main:
                return blend_weights([({f"WingWrist.{side}": 1.0}, 1.0 - best_t), (best_w, best_t)])

            if d_se <= min(d_ew, d_wd, d_dt):
                # Keep everything medial to or behind the shoulder pivot anchored on Body so folding never shears out the chest
                se_vec = we - ws
                proj_se = (pt - ws).dot(se_vec) / max(1e-6, se_vec.length_squared)
                if proj_se <= 0.0 or (abs(pt.x) < abs(ws.x) + 0.02 and pt.y > ws.y - 0.10):
                    return {"Body": 1.0}
                if proj_se < 0.26:
                    u = smoothstep(0.0, 0.26, proj_se)
                    return {"Body": 1.0 - u, f"WingShoulder.{side}": u}
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
        g_wing.add_world_capsule(thumb_mid, thumb_tip, 0.042, 0.025, HORNS, lambda *a: SHADE_HORN_TIP, wing_band)
        for ft in (f2_tip, f3_tip, f4_tip):
            g_wing.add_world_capsule(ww, ft, 0.052, 0.028, SCALES, lambda *a: SHADE_DORSAL, wing_band)
        g_wing.add_world_capsule(wd, tip_outer, 0.078, 0.056, SCALES, lambda *a: SHADE_DORSAL, wing_band)
        g_wing.add_world_box(
            (min(3.54 * sign, 3.63 * sign), 0.32, 1.21),
            (max(3.54 * sign, 3.63 * sign), 0.41, 1.29),
            HORNS, SHADE_HORN_BASE, band=9999 * int(sign),
        )
        g_wing.emit_into_bmesh(bm, color_layer, uv_layer, wing_weight, vert_weights, eyes_mat_idx=EYES)

        # Phase 1 fix: anchor membrane wingtip cleanly inside outer tip spar/horn core -> zero pinhole
        mem_tip_anchor = Vector((3.56 * sign, 0.36, 1.275))
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
            ("wd", "tip", "f2", 0.04),
            ("ww", "wd", "f2", 0.0),
            ("ww", "f2", "f3", 0.24),
            ("ww", "f3", "f4", 0.26),
            ("ww", "we", "ws", 0.0),
            ("ww", "ws", "flank", 0.0),
            ("ww", "flank", "f4", 0.20),
        ]
        emit_unified_voxel_membrane(bm, color_layer, uv_layer, MEMBRANES, anchors_w, bays, vert_weights, voxel_step=0.055)

    bm.normal_update()
    mesh = bpy.data.meshes.new("Wyvern")
    bm.to_mesh(mesh)
    bm.verts.ensure_lookup_table()
    indexed_weights = [vert_weights[v] for v in bm.verts]
    bm.free()

    for mat in mats:
        mesh.materials.append(mat)
    if mesh.color_attributes:
        mesh.color_attributes.active_color = mesh.color_attributes[0]
        mesh.color_attributes.render_color_index = 0
    if mesh.uv_layers:
        mesh.uv_layers.active_index = 0

    mesh_obj = bpy.data.objects.new("Wyvern_Mesh", mesh)
    collection.objects.link(mesh_obj)
    mesh_obj.parent = arm_obj
    mesh_obj.location = (0.0, 0.0, 0.0)
    mesh_obj.rotation_euler = (0.0, 0.0, 0.0)
    mesh_obj.scale = (1.0, 1.0, 1.0)

    vgroups = {name: mesh_obj.vertex_groups.new(name=name) for name, _ in hierarchy}
    for v_idx, w_dict in enumerate(indexed_weights):
        for bone_name, amount in w_dict.items():
            vgroups[bone_name].add([v_idx], amount, "REPLACE")

    arm_mod = mesh_obj.modifiers.new(name="Wyvern_Armature", type="ARMATURE")
    arm_mod.object = arm_obj
    arm_mod.use_vertex_groups = True
    return mesh_obj


def world_euler_to_bone_quat(arm_obj, bname, rx, ry, rz):
    pb = arm_obj.pose.bones[bname]
    world_rot = Euler((rx, ry, rz), "XYZ").to_matrix()
    bmat = pb.bone.matrix_local.to_3x3()
    return (bmat.inverted() @ world_rot @ bmat).to_quaternion()


def aim_pose_bone_quat(arm_obj, bone_name, target_world_dir, axial_roll=0.0):
    pb = arm_obj.pose.bones[bone_name]
    child = pb.children[0] if pb.children else None
    rest_dir = (child.bone.head_local - pb.bone.head_local).normalized() if child else Vector((1.0, 0.0, 0.0))
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
    return q


def segment_basis_mat(rest_dir, aim_dir, up_hint):
    """Build world rotation matrix rotating `rest_dir` cleanly onto `aim_dir` with dorsal `+Z` aligned to `up_hint`."""
    rx = rest_dir.normalized()
    up_r = Vector((0.0, 0.0, 1.0)) - rx * rx.z
    rz = up_r.normalized() if up_r.length > 1e-5 else Vector((0.0, 0.0, 1.0))
    ry = rz.cross(rx).normalized()
    r_mat = Matrix((rx, ry, rz)).transposed()

    tx = aim_dir.normalized()
    up_t = up_hint - tx * up_hint.dot(tx)
    tz = up_t.normalized() if up_t.length > 1e-5 else Vector((0.0, 0.0, 1.0))
    ty = tz.cross(tx).normalized()
    t_mat = Matrix((tx, ty, tz)).transposed()
    return t_mat @ r_mat.transposed()


def set_bone_world_basis(arm_obj, bone_name, target_world_rot_mat):
    """Set pose bone quaternion so its deformed world orientation equals `target_world_rot_mat`."""
    pb = arm_obj.pose.bones[bone_name]
    bmat = pb.bone.matrix_local.to_3x3()
    if pb.parent is not None:
        m_rel = pb.parent.bone.matrix_local.to_3x3().inverted() @ bmat
        base_rot = arm_obj.matrix_world.to_3x3() @ pb.parent.matrix.to_3x3() @ m_rel
    else:
        base_rot = arm_obj.matrix_world.to_3x3() @ bmat
    q = (base_rot.inverted() @ target_world_rot_mat @ bmat).to_quaternion()
    pb.rotation_mode = "QUATERNION"
    pb.rotation_quaternion = q
    bpy.context.view_layer.update()
    return q


def reset_pose(arm_obj):
    for pb in arm_obj.pose.bones:
        pb.location = (0.0, 0.0, 0.0)
        pb.rotation_mode = "QUATERNION"
        pb.rotation_quaternion = (1.0, 0.0, 0.0, 0.0)
        pb.scale = (1.0, 1.0, 1.0)
    bpy.context.view_layer.update()


def solve_base_wing_poses(arm_obj):
    """Compute twist-free folded-wing quaternions (~3.55 m span, Z_min > 1.0 m, zero chest crease or self-collision)."""
    reset_pose(arm_obj)
    pb_body = arm_obj.pose.bones["Body"]
    pb_body.rotation_mode = "QUATERNION"
    pb_body.rotation_quaternion = world_euler_to_bone_quat(arm_obj, "Body", 0.03, 0.0, 0.0)
    bpy.context.view_layer.update()

    fold_quats = {}
    for side, sign in (("L", 1.0), ("R", -1.0)):
        # 1) Upper arm sweeps smoothly back-upward beside ribcage (elbow tucked cleanly at X~0.96, Y~+0.06, Z~1.66)
        pb_ws = arm_obj.pose.bones[f"WingShoulder.{side}"]
        rest_se = pb_ws.children[0].bone.head_local - pb_ws.bone.head_local
        ws_pos = arm_obj.matrix_world @ pb_ws.head
        q_ws = set_bone_world_basis(
            arm_obj,
            f"WingShoulder.{side}",
            segment_basis_mat(rest_se, Vector((0.96 * sign, 0.06, 1.66)) - ws_pos, Vector((0.0, 0.0, 1.0))),
        )
        # 2) Forearm hinges smoothly forward-up beside shoulder without crossing sternum (wrist at X~1.22, Y~-0.60, Z~1.92)
        pb_we = arm_obj.pose.bones[f"WingElbow.{side}"]
        rest_ew = pb_we.children[0].bone.head_local - pb_we.bone.head_local
        we_pos = arm_obj.matrix_world @ pb_we.head
        q_we = set_bone_world_basis(
            arm_obj,
            f"WingElbow.{side}",
            segment_basis_mat(rest_ew, Vector((1.22 * sign, -0.60, 1.92)) - we_pos, Vector((0.10 * sign, 0.0, 1.0))),
        )
        # 3) Hand sweeps sleekly back (+15 deg) and lifts gently (-22 deg): ~3.60 m span, f4 clears tail, tips near shoulder height!
        w_wrist_mat = Euler((0.0, math.radians(-22.0) * sign, math.radians(15.0) * sign), "XYZ").to_matrix()
        q_ww = set_bone_world_basis(arm_obj, f"WingWrist.{side}", w_wrist_mat)
        # 4) Outer wingtip tucks neatly back (+50 deg) and level with folded cape crest
        w_digit_mat = Euler((0.0, math.radians(-16.0) * sign, math.radians(50.0) * sign), "XYZ").to_matrix()
        q_wd = set_bone_world_basis(arm_obj, f"WingDigit.{side}", w_digit_mat)

        fold_quats[f"WingShoulder.{side}"] = q_ws
        fold_quats[f"WingElbow.{side}"] = q_we
        fold_quats[f"WingWrist.{side}"] = q_ww
        fold_quats[f"WingDigit.{side}"] = q_wd

    reset_pose(arm_obj)
    return fold_quats


def measure_foot_min_z(mesh_obj):
    depsgraph = bpy.context.evaluated_depsgraph_get()
    eval_obj = mesh_obj.evaluated_get(depsgraph)
    me = eval_obj.to_mesh()
    mat_w = mesh_obj.matrix_world
    min_z = float("inf")
    for v in me.vertices:
        wp = mat_w @ v.co
        if wp.z < min_z:
            min_z = wp.z
    eval_obj.to_mesh_clear()
    return min_z


def build_wing_quats(arm_obj, lift=0.0, elbow_bend=0.0, wrist_bend=0.0, digit_bend=0.0, pitch=0.0, sweep=0.0):
    """Build smooth wing bone quaternions for flight states (Flap, Glide, Takeoff, Land)."""
    quats = {}
    for side, sign in (("L", 1.0), ("R", -1.0)):
        quats[f"WingShoulder.{side}"] = world_euler_to_bone_quat(
            arm_obj, f"WingShoulder.{side}", pitch, -lift * sign, sweep * sign
        )
        quats[f"WingElbow.{side}"] = world_euler_to_bone_quat(
            arm_obj, f"WingElbow.{side}", 0.15 * pitch, -elbow_bend * 0.35 * sign, elbow_bend * sign
        )
        quats[f"WingWrist.{side}"] = world_euler_to_bone_quat(
            arm_obj, f"WingWrist.{side}", -0.25 * wrist_bend, -wrist_bend * 0.45 * sign, wrist_bend * 0.70 * sign
        )
        quats[f"WingDigit.{side}"] = world_euler_to_bone_quat(
            arm_obj, f"WingDigit.{side}", 0.0, -digit_bend * 0.35 * sign, digit_bend * sign
        )
    return quats


def slerp_wing_quats(q_a, q_b, t):
    t = max(0.0, min(1.0, t))
    return {k: q_a[k].slerp(q_b[k], t) for k in q_a}


def eval_pose_state(arm_obj, mesh_obj, fold_quats, pose_spec):
    """Apply a pose specification dictionary onto `arm_obj` (with optional exact ground-contact Z-solve)."""
    reset_pose(arm_obj)
    rots = pose_spec["rot"]
    for bname, q in rots.items():
        pb = arm_obj.pose.bones[bname]
        pb.rotation_mode = "QUATERNION"
        pb.rotation_quaternion = q

    # Bone local +Y is World +Z -> Body vertical offset goes into `location.y`
    z_off = pose_spec.get("body_z", 0.0)
    pb_body = arm_obj.pose.bones["Body"]
    pb_body.location = (0.0, z_off, 0.0)
    bpy.context.view_layer.update()

    if pose_spec.get("ground_feet", False):
        min_z = measure_foot_min_z(mesh_obj)
        z_off -= min_z
        pb_body.location = (0.0, z_off, 0.0)
        bpy.context.view_layer.update()

    return z_off


def build_six_animations(scene, arm_obj, mesh_obj, fold_quats):
    """Author the 6 contractual Actions at 100 fps (`3.20, 1.04, 1.05, 1.02, 3.20, 1.10 s`)."""
    scene.render.fps = 100
    scene.render.fps_base = 1.0
    if arm_obj.animation_data is None:
        arm_obj.animation_data_create()

    anim_bones = [b.name for b in arm_obj.data.bones if b.name != "Root"]

    def make_pose(
        body_e=(0, 0, 0), neck_b=(0, 0, 0), neck_t=(0, 0, 0), head=(0, 0, 0), jaw=0.04,
        tail_b=(0, 0, 0), tail_m=(0, 0, 0), tail_t=(0, 0, 0),
        hips=(0, 0), knees=(0, 0), ankles=(0, 0),
        wing_quats=None, body_z=0.0, ground_feet=False
    ):
        rot = {
            "Body": world_euler_to_bone_quat(arm_obj, "Body", *body_e),
            "NeckBase": world_euler_to_bone_quat(arm_obj, "NeckBase", *neck_b),
            "NeckTip": world_euler_to_bone_quat(arm_obj, "NeckTip", *neck_t),
            "Head": world_euler_to_bone_quat(arm_obj, "Head", *head),
            "Jaw": world_euler_to_bone_quat(arm_obj, "Jaw", jaw, 0.0, 0.0),
            "TailBase": world_euler_to_bone_quat(arm_obj, "TailBase", *tail_b),
            "TailMid": world_euler_to_bone_quat(arm_obj, "TailMid", *tail_m),
            "TailTip": world_euler_to_bone_quat(arm_obj, "TailTip", *tail_t),
            "Hip.L": world_euler_to_bone_quat(arm_obj, "Hip.L", hips[0], 0.0, 0.0),
            "Knee.L": world_euler_to_bone_quat(arm_obj, "Knee.L", knees[0], 0.0, 0.0),
            "Ankle.L": world_euler_to_bone_quat(arm_obj, "Ankle.L", ankles[0], 0.0, 0.0),
            "Hip.R": world_euler_to_bone_quat(arm_obj, "Hip.R", hips[1], 0.0, 0.0),
            "Knee.R": world_euler_to_bone_quat(arm_obj, "Knee.R", knees[1], 0.0, 0.0),
            "Ankle.R": world_euler_to_bone_quat(arm_obj, "Ankle.R", ankles[1], 0.0, 0.0),
        }
        wq = fold_quats if wing_quats is None else wing_quats
        rot.update(wq)
        return {"rot": rot, "body_z": body_z, "ground_feet": ground_feet}

    # Shared anchor poses for clean transitions across clips!
    def idle_spec(u):
        th = math.tau * u
        s = math.sin(th)
        c = math.cos(th)
        breath = 0.5 * (1.0 - c)  # 0 -> 1 -> 0 smooth periodic
        # Keep wings folded near 3.53 m with gentle breathing lift
        wq = {}
        for side, sign in (("L", 1.0), ("R", -1.0)):
            wq[f"WingShoulder.{side}"] = fold_quats[f"WingShoulder.{side}"] @ world_euler_to_bone_quat(arm_obj, f"WingShoulder.{side}", -0.02 * breath, -0.025 * breath * sign, 0.0)
            wq[f"WingElbow.{side}"] = fold_quats[f"WingElbow.{side}"]
            wq[f"WingWrist.{side}"] = fold_quats[f"WingWrist.{side}"] @ world_euler_to_bone_quat(arm_obj, f"WingWrist.{side}", 0.03 * breath, 0.0, 0.0)
            wq[f"WingDigit.{side}"] = fold_quats[f"WingDigit.{side}"]
        return make_pose(
            body_e=(0.03 - 0.018 * breath, 0.0, 0.015 * s),
            neck_b=(-0.03 + 0.035 * breath, 0.0, 0.05 * s),
            neck_t=(0.03 - 0.03 * breath, 0.0, -0.03 * s),
            head=(0.01 + 0.04 * s, 0.02 * s, 0.06 * math.sin(th * 0.5 + 0.0) * 0.0 + 0.06 * s),
            jaw=0.05 + 0.05 * breath,
            tail_b=(0.01 * breath, 0.0, 0.14 * s),
            tail_m=(-0.02 * breath, 0.0, 0.22 * s),
            tail_t=(-0.03 * breath, 0.0, 0.28 * s),
            hips=(-0.04 + 0.02 * breath, -0.04 + 0.02 * breath),
            knees=(0.08 - 0.04 * breath, 0.08 - 0.04 * breath),
            ankles=(-0.07 + 0.035 * breath, -0.07 + 0.035 * breath),
            wing_quats=wq,
            ground_feet=True,
        )

    def walk_spec(u):
        th = math.tau * u
        sL = math.sin(th)
        cL = math.cos(th)
        sR = -sL
        cR = -cL
        # Bipedal stride: swing knee bends when moving forward (c > 0), ankle keeps foot flat on stance
        hipL = -0.34 * sL
        hipR = -0.34 * sR
        kneeL = 0.08 + 0.44 * max(0.0, cL)
        kneeR = 0.08 + 0.44 * max(0.0, cR)
        ankL = -0.06 - 0.18 * max(0.0, cL) + 0.22 * sL
        ankR = -0.06 - 0.18 * max(0.0, cR) + 0.22 * sR
        stride_bounce = abs(math.cos(th))  # peaks at mid-stance, dips during double-support
        wq = {}
        for side, sign in (("L", 1.0), ("R", -1.0)):
            wq[f"WingShoulder.{side}"] = fold_quats[f"WingShoulder.{side}"] @ world_euler_to_bone_quat(arm_obj, f"WingShoulder.{side}", -0.03 * stride_bounce, -0.04 * stride_bounce * sign, 0.03 * sL * sign)
            wq[f"WingElbow.{side}"] = fold_quats[f"WingElbow.{side}"]
            wq[f"WingWrist.{side}"] = fold_quats[f"WingWrist.{side}"] @ world_euler_to_bone_quat(arm_obj, f"WingWrist.{side}", 0.04 * stride_bounce, 0.0, 0.0)
            wq[f"WingDigit.{side}"] = fold_quats[f"WingDigit.{side}"]
        return make_pose(
            body_e=(0.05 + 0.03 * math.cos(2 * th), 0.04 * sL, 0.04 * cL),
            neck_b=(-0.05 - 0.04 * math.cos(2 * th), 0.0, -0.05 * cL),
            neck_t=(0.04 + 0.03 * math.cos(2 * th), 0.0, 0.03 * cL),
            head=(-0.02 + 0.03 * math.sin(2 * th), -0.03 * sL, 0.02 * cL),
            jaw=0.06 + 0.03 * stride_bounce,
            tail_b=(-0.02 * math.cos(2 * th), 0.0, 0.22 * sL),
            tail_m=(0.02 * math.cos(2 * th), 0.0, 0.34 * sL),
            tail_t=(0.03 * math.cos(2 * th), 0.0, 0.44 * sL),
            hips=(hipL, hipR),
            knees=(kneeL, kneeR),
            ankles=(ankL, ankR),
            wing_quats=wq,
            ground_feet=True,
        )

    def flap_spec(u):
        th = math.tau * u
        s = math.sin(th)
        c = math.cos(th)
        # u=0: soaring wide-open entry (matching Glide & Takeoff end) -> downstroke (u=0.25..0.5) -> lifted upstroke (u=0.75..1.0)
        lift = 0.34 * c - 0.38 * s + 0.10
        wrist_fold = 0.24 * max(0.0, s) - 0.14 * max(0.0, -s)
        elbow_fold = 0.20 * max(0.0, s) - 0.08 * max(0.0, -s)
        digit_fold = 0.20 * max(0.0, s) - 0.08 * max(0.0, -s)
        wq = build_wing_quats(
            arm_obj,
            lift=lift,
            elbow_bend=elbow_fold,
            wrist_bend=wrist_fold,
            digit_bend=digit_fold,
            pitch=0.12 * s,
            sweep=0.06 * s - 0.04,
        )
        return make_pose(
            body_e=(-0.05 - 0.06 * s, 0.0, 0.0),
            neck_b=(0.04 + 0.05 * s, 0.0, 0.0),
            neck_t=(-0.04 - 0.04 * s, 0.0, 0.0),
            head=(0.02 + 0.04 * s, 0.0, 0.0),
            jaw=0.06 + 0.08 * max(0.0, s),
            tail_b=(0.04 * s, 0.0, 0.08 * s),
            tail_m=(0.06 * s, 0.0, 0.14 * s),
            tail_t=(0.08 * s, 0.0, 0.18 * s),
            hips=(0.38, 0.38),
            knees=(0.58, 0.58),
            ankles=(0.28, 0.28),
            wing_quats=wq,
            body_z=0.34 - 0.09 * s,
            ground_feet=False,
        )

    def glide_spec(u):
        th = math.tau * u
        s = math.sin(th)
        c = math.cos(th)
        # Wide majestic raptor dihedral (~7.2 m full span) cleanly elevated above torso & tail
        wq = build_wing_quats(
            arm_obj,
            lift=0.28 + 0.05 * s,
            elbow_bend=-0.04 + 0.03 * c,
            wrist_bend=-0.05 + 0.04 * s,
            digit_bend=0.06 + 0.04 * s,
            pitch=-0.03 + 0.03 * c,
            sweep=-0.06 + 0.03 * s,
        )
        return make_pose(
            body_e=(-0.04 + 0.025 * s, 0.035 * math.sin(2 * th), 0.02 * s),
            neck_b=(0.04 - 0.02 * s, 0.0, 0.04 * s),
            neck_t=(-0.04 + 0.02 * s, 0.0, -0.02 * s),
            head=(0.02 + 0.03 * c, 0.03 * s, 0.05 * s),
            jaw=0.05 + 0.03 * (1 - c) * 0.5,
            tail_b=(-0.02 * s, 0.0, 0.12 * s),
            tail_m=(0.02 * s, 0.0, 0.20 * s),
            tail_t=(0.03 * s, 0.0, 0.26 * s),
            hips=(0.38, 0.38),
            knees=(0.56, 0.56),
            ankles=(0.28, 0.28),
            wing_quats=wq,
            body_z=0.32 + 0.05 * s,
            ground_feet=False,
        )

    crouch_body_z = eval_pose_state(
        arm_obj, mesh_obj, fold_quats,
        make_pose(
            body_e=(0.17, 0.0, 0.0), neck_b=(0.12, 0.0, 0.0), neck_t=(-0.11, 0.0, 0.0),
            head=(-0.07, 0.0, 0.0), jaw=0.27, tail_b=(0.0, 0.0, 0.0), tail_m=(-0.06, 0.0, 0.0),
            tail_t=(-0.08, 0.0, 0.0), hips=(-0.36, -0.36), knees=(0.64, 0.64), ankles=(-0.35, -0.35),
            ground_feet=True,
        )
    )

    def takeoff_spec(u):
        p_flap0 = flap_spec(0.0)
        if u <= 0.25:
            t = smoothstep(0.0, 0.25, u)
            wq = slerp_wing_quats(fold_quats, build_wing_quats(arm_obj, lift=0.25, elbow_bend=0.18, wrist_bend=0.18, sweep=0.20), 0.45 * t)
            return make_pose(
                body_e=(0.03 + 0.14 * t, 0.0, 0.0),
                neck_b=(-0.03 + 0.15 * t, 0.0, 0.0),
                neck_t=(0.03 - 0.14 * t, 0.0, 0.0),
                head=(0.01 - 0.08 * t, 0.0, 0.0),
                jaw=0.05 + 0.22 * t,
                tail_b=(0.0, 0.0, 0.0),
                tail_m=(-0.06 * t, 0.0, 0.0),
                tail_t=(-0.08 * t, 0.0, 0.0),
                hips=(-0.04 - 0.32 * t, -0.04 - 0.32 * t),
                knees=(0.08 + 0.56 * t, 0.08 + 0.56 * t),
                ankles=(-0.07 - 0.28 * t, -0.07 - 0.28 * t),
                wing_quats=wq,
                ground_feet=True,
            )
        if u <= 0.60:
            t = smoothstep(0.25, 0.60, u)
            wq_mid = build_wing_quats(arm_obj, lift=0.48, elbow_bend=0.04, wrist_bend=0.06, sweep=-0.08)
            wq = slerp_wing_quats(
                slerp_wing_quats(fold_quats, build_wing_quats(arm_obj, lift=0.25, elbow_bend=0.18, wrist_bend=0.18, sweep=0.20), 0.45),
                wq_mid, t,
            )
            return make_pose(
                body_e=(0.17 - 0.35 * t, 0.0, 0.0),
                neck_b=(0.12 - 0.12 * t, 0.0, 0.0),
                neck_t=(-0.11 + 0.08 * t, 0.0, 0.0),
                head=(-0.07 + 0.12 * t, 0.0, 0.0),
                jaw=0.27 - 0.10 * t,
                hips=(-0.36 + 0.66 * t, -0.36 + 0.66 * t),
                knees=(0.64 - 0.22 * t, 0.64 - 0.22 * t),
                ankles=(-0.35 + 0.58 * t, -0.35 + 0.58 * t),
                wing_quats=wq,
                body_z=crouch_body_z + (0.36 - crouch_body_z) * t,
                ground_feet=False,
            )
        t = smoothstep(0.60, 1.00, u)
        wq_mid = build_wing_quats(arm_obj, lift=0.48, elbow_bend=0.04, wrist_bend=0.06, sweep=-0.08)
        wq_flap0 = {k: p_flap0["rot"][k] for k in wq_mid}
        wq = slerp_wing_quats(wq_mid, wq_flap0, t)
        return make_pose(
            body_e=(-0.18 + 0.13 * t, 0.0, 0.0),
            neck_b=(0.0 + 0.04 * t, 0.0, 0.0),
            neck_t=(-0.03 - 0.01 * t, 0.0, 0.0),
            head=(0.05 - 0.03 * t, 0.0, 0.0),
            jaw=0.17 - 0.11 * t,
            hips=(0.30 + 0.08 * t, 0.30 + 0.08 * t),
            knees=(0.42 + 0.16 * t, 0.42 + 0.16 * t),
            ankles=(0.23 + 0.05 * t, 0.23 + 0.05 * t),
            wing_quats=wq,
            body_z=0.36 - 0.02 * t,
            ground_feet=False,
        )

    touch_body_z = eval_pose_state(
        arm_obj, mesh_obj, fold_quats,
        make_pose(
            body_e=(-0.22, 0.0, 0.0), neck_b=(0.12, 0.0, 0.0), neck_t=(-0.10, 0.0, 0.0),
            head=(0.12, 0.0, 0.0), jaw=0.18, hips=(-0.20, -0.20), knees=(0.40, 0.40), ankles=(-0.14, -0.14),
            ground_feet=True,
        )
    )

    def land_spec(u):
        p_glide0 = glide_spec(0.0)
        wq_glide0 = {k: p_glide0["rot"][k] for k in fold_quats}
        wq_flare = build_wing_quats(arm_obj, lift=0.38, elbow_bend=0.12, wrist_bend=0.10, pitch=-0.26, sweep=-0.24)
        wq_tuck_mid = build_wing_quats(arm_obj, lift=0.32, elbow_bend=0.22, wrist_bend=0.28, pitch=-0.08, sweep=0.15)
        if u <= 0.42:
            t = smoothstep(0.0, 0.42, u)
            wq = slerp_wing_quats(wq_glide0, wq_flare, t)
            return make_pose(
                body_e=(-0.04 - 0.18 * t, 0.0, 0.0),
                neck_b=(0.04 + 0.08 * t, 0.0, 0.0),
                neck_t=(-0.04 - 0.06 * t, 0.0, 0.0),
                head=(0.02 + 0.10 * t, 0.0, 0.0),
                jaw=0.05 + 0.13 * t,
                hips=(0.38 - 0.58 * t, 0.38 - 0.58 * t),
                knees=(0.56 - 0.16 * t, 0.56 - 0.16 * t),
                ankles=(0.28 - 0.42 * t, 0.28 - 0.42 * t),
                wing_quats=wq,
                body_z=0.32 + (touch_body_z - 0.32) * t,
                ground_feet=False,
            )
        if u <= 0.68:
            t = smoothstep(0.42, 0.68, u)
            wq = slerp_wing_quats(wq_flare, slerp_wing_quats(wq_tuck_mid, fold_quats, 0.45), t)
            return make_pose(
                body_e=(-0.22 + 0.30 * t, 0.0, 0.0),
                neck_b=(0.12 - 0.10 * t, 0.0, 0.0),
                neck_t=(-0.10 + 0.08 * t, 0.0, 0.0),
                head=(0.12 - 0.08 * t, 0.0, 0.0),
                jaw=0.18 - 0.08 * t,
                hips=(-0.20 - 0.12 * t, -0.20 - 0.12 * t),
                knees=(0.40 + 0.20 * t, 0.40 + 0.20 * t),
                ankles=(-0.14 - 0.14 * t, -0.14 - 0.14 * t),
                wing_quats=wq,
                ground_feet=True,
            )
        t = smoothstep(0.68, 1.00, u)
        wq_start = slerp_wing_quats(wq_tuck_mid, fold_quats, 0.45)
        wq = slerp_wing_quats(wq_start, fold_quats, t)
        return make_pose(
            body_e=(0.08 - 0.05 * t, 0.0, 0.0),
            neck_b=(0.02 - 0.05 * t, 0.0, 0.0),
            neck_t=(-0.02 + 0.05 * t, 0.0, 0.0),
            head=(0.04 - 0.03 * t, 0.0, 0.0),
            jaw=0.10 - 0.05 * t,
            hips=(-0.32 + 0.28 * t, -0.32 + 0.28 * t),
            knees=(0.60 - 0.52 * t, 0.60 - 0.52 * t),
            ankles=(-0.28 + 0.21 * t, -0.28 + 0.21 * t),
            wing_quats=wq,
            ground_feet=True,
        )

    specs = [
        ("IdleGround", 320, 32, idle_spec, True),
        ("Walk", 104, 26, walk_spec, True),
        ("Takeoff", 105, 21, takeoff_spec, False),
        ("Flap", 102, 34, flap_spec, True),
        ("Glide", 320, 32, glide_spec, True),
        ("Land", 110, 22, land_spec, False),
    ]

    clip_reports = {}
    for clip_name, end_frame, sample_count, fn_spec, is_loop in specs:
        act = bpy.data.actions.new(name=clip_name)
        act.use_fake_user = True
        arm_obj.animation_data.action = act

        frames = sorted({round(end_frame * i / sample_count) for i in range(sample_count + 1)})
        min_z_clip = float("inf")
        min_mem_z_clip = float("inf")
        first_rot = None
        last_rot = None
        first_z = None
        last_z = None

        mem_idx = next((i for i, m in enumerate(mesh_obj.data.materials) if m and m.name == "Membranes"), 3)

        for fi in frames:
            u = fi / end_frame
            spec = fn_spec(u)
            body_z = eval_pose_state(arm_obj, mesh_obj, fold_quats, spec)
            # Measure clip ground & membrane clearance
            depsgraph = bpy.context.evaluated_depsgraph_get()
            ev = mesh_obj.evaluated_get(depsgraph)
            me = ev.to_mesh()
            mat_w = mesh_obj.matrix_world
            mem_verts = set()
            for p in me.polygons:
                if p.material_index == mem_idx:
                    mem_verts.update(p.vertices)
            for vi, v in enumerate(me.vertices):
                wz = (mat_w @ v.co).z
                min_z_clip = min(min_z_clip, wz)
                if vi in mem_verts:
                    min_mem_z_clip = min(min_mem_z_clip, wz)
            ev.to_mesh_clear()

            # Keyframe Body location (vertical only, zero horizontal root motion!) and all non-Root bone rotations
            pb_body = arm_obj.pose.bones["Body"]
            pb_body.keyframe_insert(data_path="location", frame=fi)
            for bname in anim_bones:
                pb = arm_obj.pose.bones[bname]
                pb.keyframe_insert(data_path="rotation_quaternion", frame=fi)

            cur_rots = {bname: tuple(arm_obj.pose.bones[bname].rotation_quaternion) for bname in anim_bones}
            if first_rot is None:
                first_rot = cur_rots
                first_z = body_z
            last_rot = cur_rots
            last_z = body_z

        # Use linear interpolation across dense keys (avoid Bezier overshoot / match glTF sampling)
        for fc in act.fcurves:
            for kp in fc.keyframe_points:
                kp.interpolation = "LINEAR"

        seam_rot_err = max(
            abs(abs(sum(a * b for a, b in zip(first_rot[b], last_rot[b]))) - 1.0)
            for b in anim_bones
        ) if is_loop else None

        clip_reports[clip_name] = {
            "frames": [0, end_frame],
            "duration_s": round(end_frame / 100.0, 4),
            "keyframe_count": len(frames),
            "loop": is_loop,
            "min_world_z_m": round(min_z_clip, 4),
            "min_membrane_z_m": round(min_mem_z_clip, 4),
            "loop_seam_quat_error": round(seam_rot_err, 7) if seam_rot_err is not None else None,
            "loop_seam_body_z_error_m": round(abs(last_z - first_z), 7) if is_loop else None,
        }

    reset_pose(arm_obj)
    return clip_reports


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
    uvl = bm.loops.layers.uv.new("UVMap")
    vw = {}
    g.emit_into_bmesh(bm, cl, uvl, lambda pt: {}, vw, eyes_mat_idx=-1)
    mesh = bpy.data.meshes.new("Human_180cm_Mesh")
    bm.to_mesh(mesh)
    bm.free()
    mesh.materials.append(mat_human)
    obj = bpy.data.objects.new("Human_ScaleRef_180cm", mesh)
    obj.location = Vector(location)
    collection.objects.link(obj)
    return obj


def setup_stage(scene):
    scene.unit_settings.system = "METRIC"
    scene.unit_settings.scale_length = 1.0
    scene.render.engine = "BLENDER_EEVEE_NEXT"
    scene.render.resolution_x = 1280
    scene.render.resolution_y = 800
    scene.render.resolution_percentage = 100
    scene.render.image_settings.file_format = "PNG"
    scene.view_settings.view_transform = "Standard"
    scene.view_settings.look = "None"

    world = bpy.data.worlds.new(f"World_{scene.name}")
    scene.world = world
    world.use_nodes = True
    bg = world.node_tree.nodes.get("Background")
    if bg:
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


def evaluate_action_at_time(scene, arm_obj, action_name, t_sec):
    """Evaluate an Action at exact time `t_sec` (seconds) via Blender scene frame."""
    arm_obj.animation_data.action = bpy.data.actions[action_name]
    scene.frame_set(int(round(t_sec * 100.0)))
    bpy.context.view_layer.update()


def simulate_transition_blend(scene, arm_obj, mesh_obj, action_a, t_a, action_b, t_b, blend_frac):
    """Simulate Bevy's 220 ms native crossfade blend between two Actions at `blend_frac` in [0..1]."""
    evaluate_action_at_time(scene, arm_obj, action_a, t_a)
    pose_a_rot = {pb.name: pb.rotation_quaternion.copy() for pb in arm_obj.pose.bones}
    pose_a_loc = arm_obj.pose.bones["Body"].location.copy()

    evaluate_action_at_time(scene, arm_obj, action_b, t_b)
    pose_b_rot = {pb.name: pb.rotation_quaternion.copy() for pb in arm_obj.pose.bones}
    pose_b_loc = arm_obj.pose.bones["Body"].location.copy()

    arm_obj.animation_data.action = None
    for pb in arm_obj.pose.bones:
        pb.rotation_mode = "QUATERNION"
        pb.rotation_quaternion = pose_a_rot[pb.name].slerp(pose_b_rot[pb.name], blend_frac)
    arm_obj.pose.bones["Body"].location = pose_a_loc.lerp(pose_b_loc, blend_frac)
    bpy.context.view_layer.update()

    depsgraph = bpy.context.evaluated_depsgraph_get()
    ev = mesh_obj.evaluated_get(depsgraph)
    me = ev.to_mesh()
    mat_w = mesh_obj.matrix_world
    mem_idx = next((i for i, m in enumerate(mesh_obj.data.materials) if m and m.name == "Membranes"), 3)
    min_z = float("inf")
    mem_min_z = float("inf")
    mem_verts = set()
    for p in me.polygons:
        if p.material_index == mem_idx:
            mem_verts.update(p.vertices)
    for vi, v in enumerate(me.vertices):
        wz = (mat_w @ v.co).z
        min_z = min(min_z, wz)
        if vi in mem_verts:
            mem_min_z = min(mem_min_z, wz)
    ev.to_mesh_clear()
    return {"min_world_z_m": round(min_z, 4), "min_membrane_z_m": round(mem_min_z, 4)}


def render_transition_sequence_video(scene, arm_obj, cam_obj, out_mp4):
    """Render a smooth 25 fps preview sequence of the full flight state machine into MP4."""
    # Sequence: IdleGround(0.8s) -> Walk(1.04s) -> Takeoff(1.05s) -> Flap(1.02s) -> Glide(1.0s) -> Flap(0.5s) -> Land(1.10s) -> IdleGround(0.8s)
    segments = [
        ("IdleGround", 0.0, 0.80, "IdleGround", 0.80, 0.0),
        ("Walk", 0.0, 1.04, "IdleGround", 0.80, 0.22),
        ("Takeoff", 0.0, 1.05, "Walk", 1.04, 0.22),
        ("Flap", 0.0, 1.02, "Takeoff", 1.05, 0.22),
        ("Glide", 0.0, 1.00, "Flap", 1.02, 0.22),
        ("Flap", 0.0, 0.51, "Glide", 1.00, 0.22),
        ("Land", 0.0, 1.10, "Flap", 0.51, 0.22),
        ("IdleGround", 0.0, 0.80, "Land", 1.10, 0.22),
    ]

    seq_fps = 25
    old_format = scene.render.image_settings.file_format
    old_res_x, old_res_y = scene.render.resolution_x, scene.render.resolution_y
    scene.render.resolution_x = 960
    scene.render.resolution_y = 540
    scene.render.fps = seq_fps
    scene.render.image_settings.file_format = "FFMPEG"
    scene.render.ffmpeg.format = "MPEG4"
    scene.render.ffmpeg.codec = "H264"
    scene.render.ffmpeg.constant_rate_factor = "MEDIUM"

    eye, tgt = (6.0, -6.5, 3.4), (0.0, 0.25, 1.25)
    cam_obj.location = Vector(eye)
    cam_obj.rotation_euler = (Vector(tgt) - Vector(eye)).to_track_quat("-Z", "Y").to_euler()
    cam_obj.data.type = "PERSP"
    cam_obj.data.lens = 36.0

    # Render frames directly into a temp dir, then pack via Blender Video Sequencer to MP4
    import tempfile
    with tempfile.TemporaryDirectory() as tmpdir:
        tmp = Path(tmpdir)
        frame_paths = []
        frame_idx = 0
        for act_cur, t0, t1, act_prev, prev_t, fade_dur in segments:
            n_frames = max(2, int(round((t1 - t0) * seq_fps)))
            for k in range(n_frames):
                t_local = t0 + (t1 - t0) * (k / n_frames)
                if fade_dur > 0.0 and t_local < fade_dur:
                    alpha = t_local / fade_dur
                    evaluate_action_at_time(scene, arm_obj, act_prev, prev_t)
                    pa = {pb.name: pb.rotation_quaternion.copy() for pb in arm_obj.pose.bones}
                    la = arm_obj.pose.bones["Body"].location.copy()
                    evaluate_action_at_time(scene, arm_obj, act_cur, t_local)
                    pb_r = {pb.name: pb.rotation_quaternion.copy() for pb in arm_obj.pose.bones}
                    lb = arm_obj.pose.bones["Body"].location.copy()
                    arm_obj.animation_data.action = None
                    for pb in arm_obj.pose.bones:
                        pb.rotation_mode = "QUATERNION"
                        pb.rotation_quaternion = pa[pb.name].slerp(pb_r[pb.name], alpha)
                    arm_obj.pose.bones["Body"].location = la.lerp(lb, alpha)
                    bpy.context.view_layer.update()
                else:
                    evaluate_action_at_time(scene, arm_obj, act_cur, t_local)

                fp = tmp / f"f_{frame_idx:04d}.png"
                scene.render.image_settings.file_format = "PNG"
                scene.render.filepath = str(fp)
                bpy.ops.render.render(write_still=True)
                frame_paths.append(fp)
                frame_idx += 1

        # Assemble PNGs into MP4 via temporary VSE scene
        sc_vid = bpy.data.scenes.new("SeqRender")
        sc_vid.sequence_editor_create()
        sc_vid.render.resolution_x = 960
        sc_vid.render.resolution_y = 540
        sc_vid.render.fps = seq_fps
        sc_vid.render.image_settings.file_format = "FFMPEG"
        sc_vid.render.ffmpeg.format = "MPEG4"
        sc_vid.render.ffmpeg.codec = "H264"
        sc_vid.render.ffmpeg.constant_rate_factor = "MEDIUM"
        sc_vid.render.filepath = str(out_mp4)
        sc_vid.frame_start = 1
        sc_vid.frame_end = len(frame_paths)

        seq = sc_vid.sequence_editor.sequences.new_image(
            name="WyvernSeq", filepath=str(frame_paths[0]), channel=1, frame_start=1
        )
        for p in frame_paths[1:]:
            seq.elements.append(p.name)

        bpy.context.window.scene = sc_vid
        bpy.ops.render.render(animation=True)
        bpy.data.scenes.remove(sc_vid)

    bpy.context.window.scene = scene
    scene.render.fps = 100
    scene.render.resolution_x = old_res_x
    scene.render.resolution_y = old_res_y
    scene.render.image_settings.file_format = old_format
    return len(frame_paths)


def write_art_manifest(path):
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
        "palettes": PALETTES,
    }
    path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--blend", type=Path, default=DEFAULT_BLEND)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--previews", type=Path, default=DEFAULT_PREVIEW_DIR)
    parser.add_argument("--metrics", type=Path, default=DEFAULT_METRICS)
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else [])

    # 1. Generate lightweight crisp PNG textures under assets/creatures/wyvern_art/textures/
    tex_files = generate_png_textures(TEX_DIR)

    bpy.ops.wm.read_factory_settings(use_empty=True)

    # 2. Create 4 PBR materials referencing //textures/... PNGs (neutral albedo for Bevy palettes)
    mat_scales = make_textured_material("Scales", "wyvern_scales_albedo.png", "wyvern_scales_normal.png", roughness=0.76)
    mat_horns = make_textured_material("Horns", "wyvern_horns_albedo.png", "wyvern_horns_normal.png", roughness=0.64)
    mat_eyes = make_textured_material("Eyes", "wyvern_eyes_albedo.png", None, roughness=0.26)
    mat_mem = make_textured_material("Membranes", "wyvern_membranes_albedo.png", "wyvern_membranes_normal.png", roughness=0.84, double_sided=True)
    mats = [mat_scales, mat_horns, mat_eyes, mat_mem]

    # 3. Clean exportable scene `Wyvern`
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

    # 4. Author the 6 contractual Actions
    fold_quats = solve_base_wing_poses(arm_obj)
    clip_reports = build_six_animations(scene_wyvern, arm_obj, mesh_obj, fold_quats)

    # Restore exact neutral bind pose and save .blend BEFORE preview shading
    arm_obj.animation_data.action = bpy.data.actions["IdleGround"]
    reset_pose(arm_obj)
    args.blend.parent.mkdir(parents=True, exist_ok=True)
    bpy.context.window.scene = scene_wyvern
    # Save compressed .blend with clean neutral materials & relative //textures/... paths
    bpy.ops.wm.save_as_mainfile(filepath=str(args.blend), compress=True)
    write_art_manifest(args.manifest)

    # 5. Presentation scene for diagnostic captures (poses, 220ms transitions, 3 palettes, sequence MP4)
    scene_pres = bpy.data.scenes.new("Presentation")
    bpy.context.window.scene = scene_pres
    scene_pres.render.fps = 100
    cam_pres = setup_stage(scene_pres)
    scene_pres.collection.children.link(col_wyvern)
    col_human = bpy.data.collections.new("ScaleReference_Human_180cm")
    scene_pres.collection.children.link(col_human)
    create_human_reference(col_human, location=(2.20, -2.35, 0.0))
    bpy.context.view_layer.update()

    args.previews.mkdir(parents=True, exist_ok=True)
    render_times = {}

    # Apply Jade preview tint for Blender visual inspection renders
    set_preview_palette(mats, "Jade")

    cam_front = ((0.0, -9.5, 1.45), (0.15, 0.0, 1.35), True, 8.4, 36.0)
    cam_profile = ((-9.5, 0.45, 1.45), (0.0, 0.45, 1.35), True, 7.6, 36.0)
    cam_top = ((0.0, 0.50, 11.5), (0.0, 0.50, 1.2), True, 10.6, 36.0)
    cam_3q = ((6.2, -6.8, 3.6), (0.2, 0.25, 1.25), False, 8.4, 36.0)

    # Render Bind pose (Open Wings) to verify mem_tip_anchor fix & textures
    arm_obj.animation_data.action = None
    reset_pose(arm_obj)
    render_times["c2_bind_front.png"] = render_view(scene_pres, cam_pres, *cam_front[:2], args.previews / "c2_bind_front.png", ortho=True, ortho_scale=8.4)
    render_times["c2_bind_top.png"] = render_view(scene_pres, cam_pres, *cam_top[:2], args.previews / "c2_bind_top.png", ortho=True, ortho_scale=10.6)

    # Render key poses from the 6 Actions
    pose_shots = [
        ("c2_idle_00_front.png", "IdleGround", 0.0, cam_front),
        ("c2_idle_00_profile.png", "IdleGround", 0.0, cam_profile),
        ("c2_walk_25_profile.png", "Walk", 0.26, cam_profile),
        ("c2_walk_75_profile.png", "Walk", 0.78, cam_profile),
        ("c2_takeoff_50_3q.png", "Takeoff", 0.52, cam_3q),
        ("c2_flap_25_front.png", "Flap", 0.25, cam_front),
        ("c2_flap_50_profile.png", "Flap", 0.51, cam_profile),
        ("c2_flap_75_3q.png", "Flap", 0.76, cam_3q),
        ("c2_glide_50_3q.png", "Glide", 1.60, cam_3q),
        ("c2_land_50_profile.png", "Land", 0.55, cam_profile),
    ]
    for fname, act_name, t_s, cfg in pose_shots:
        evaluate_action_at_time(scene_pres, arm_obj, act_name, t_s)
        render_times[fname] = render_view(
            scene_pres, cam_pres, cfg[0], cfg[1], args.previews / fname,
            ortho=cfg[2], ortho_scale=cfg[3], lens=cfg[4]
        )

    # Render 3 Palettes (Jade, Braise, Obsidienne) on iconic open-wing Flap pose 3/4 view
    evaluate_action_at_time(scene_pres, arm_obj, "Flap", 0.76)
    for pal_name in ("Jade", "Braise", "Obsidienne"):
        set_preview_palette(mats, pal_name)
        fname = f"c2_palette_{pal_name.lower()}.png"
        render_times[fname] = render_view(
            scene_pres, cam_pres, cam_3q[0], cam_3q[1], args.previews / fname,
            ortho=False, lens=36.0
        )

    # Restore Jade for sequence & transition checks
    set_preview_palette(mats, "Jade")

    # Simulate Bevy 220 ms native crossfade midpoints (blend_frac=0.5) & render key transition snapshots
    transitions = [
        ("IdleGround->Walk", "IdleGround", 0.80, "Walk", 0.11),
        ("Walk->IdleGround", "Walk", 0.52, "IdleGround", 0.11),
        ("IdleGround->Takeoff", "IdleGround", 1.60, "Takeoff", 0.11),
        ("Takeoff->Flap", "Takeoff", 1.05, "Flap", 0.11),
        ("Flap->Glide", "Flap", 1.02, "Glide", 0.11),
        ("Glide->Flap", "Glide", 1.60, "Flap", 0.11),
        ("Flap->Land", "Flap", 1.02, "Land", 0.11),
        ("Land->IdleGround", "Land", 1.10, "IdleGround", 0.11),
    ]
    transition_checks = {}
    for label, a_name, t_a, b_name, t_b in transitions:
        transition_checks[label] = simulate_transition_blend(
            scene_pres, arm_obj, mesh_obj, a_name, t_a, b_name, t_b, 0.5
        )
        if label in ("Takeoff->Flap", "Land->IdleGround"):
            sname = "c2_trans_" + label.lower().replace("->", "_to_") + ".png"
            render_times[sname] = render_view(
                scene_pres, cam_pres, cam_3q[0], cam_3q[1], args.previews / sname, ortho=False, lens=36.0
            )

    # Render full sequence preview video (MP4)
    mp4_path = args.previews / "wyvern_sequence_preview.mp4"
    t0_vid = time.perf_counter()
    seq_frames = render_transition_sequence_video(scene_pres, arm_obj, cam_pres, mp4_path)
    render_times["wyvern_sequence_preview.mp4"] = round(time.perf_counter() - t0_vid, 3)

    # Clear preview palette tint & save final wyvern.blend cleanly (NOTE: don't alter Wyvern scene!)
    clear_preview_palette(mats)
    arm_obj.animation_data.action = bpy.data.actions["IdleGround"]
    reset_pose(arm_obj)
    bpy.context.window.scene = scene_wyvern
    bpy.ops.wm.save_as_mainfile(filepath=str(args.blend), compress=True)

    payload = {
        "pass": "ART-C2",
        "blender_version": bpy.app.version_string,
        "textures": tex_files,
        "clips": clip_reports,
        "transition_220ms_midpoints": transition_checks,
        "sequence_video_frames": seq_frames,
        "render_times_sec": render_times,
        "total_render_time_sec": round(sum(render_times.values()), 3),
    }
    args.metrics.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(payload, indent=2))


if __name__ == "__main__":
    main()
