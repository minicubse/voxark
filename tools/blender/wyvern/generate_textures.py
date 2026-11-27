"""Generate lightweight, crisp modern-voxel PNG textures for wyvern_art.

Produces neutral-luminance albedo maps (designed to multiply cleanly against
Jade, Braise, and Obsidienne sRGB palette factors in Bevy without crushing hue)
and matching tangent-space normal maps under assets/creatures/wyvern_art/textures/.
"""

import math
import struct
import zlib
from pathlib import Path


DEFAULT_TEX_DIR = Path(__file__).resolve().parents[3] / "assets/creatures/wyvern_art/textures"


def write_png_rgba(path: Path, width: int, height: int, pixels_bytes: bytearray):
    """Write an 8-bit RGBA PNG using Python stdlib zlib/struct."""
    def chunk(tag: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    stride = width * 4
    raw = bytearray()
    for y in range(height):
        raw.append(0)  # filter type None
        start = y * stride
        raw.extend(pixels_bytes[start:start + stride])

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    idat = zlib.compress(bytes(raw), level=9)
    png_bytes = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", idat) + chunk(b"IEND", b"")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(png_bytes)


def height_to_normal_png(path: Path, width: int, height: int, heights, strength=2.2):
    """Convert a float height array [0..1] into a tileable tangent-space normal PNG."""
    buf = bytearray(width * height * 4)
    for y in range(height):
        ym = (y - 1) % height
        yp = (y + 1) % height
        row_off = y * width
        for x in range(width):
            xm = (x - 1) % width
            xp = (x + 1) % width
            dx = (heights[row_off + xp] - heights[row_off + xm]) * strength
            # OpenGL / glTF +Y normal points up (toward smaller PNG row index ym)
            dy = (heights[ym * width + x] - heights[yp * width + x]) * strength
            nx, ny, nz = -dx, -dy, 1.0
            inv = 1.0 / math.sqrt(nx * nx + ny * ny + nz * nz)
            nx *= inv
            ny *= inv
            nz *= inv
            idx = (row_off + x) * 4
            buf[idx] = max(0, min(255, int((nx * 0.5 + 0.5) * 255.0 + 0.5)))
            buf[idx + 1] = max(0, min(255, int((ny * 0.5 + 0.5) * 255.0 + 0.5)))
            buf[idx + 2] = max(0, min(255, int((nz * 0.5 + 0.5) * 255.0 + 0.5)))
            buf[idx + 3] = 255
    write_png_rgba(path, width, height, buf)


def hash2d(ix: int, iy: int, seed: int = 1337) -> float:
    n = (ix * 374761393 + iy * 668265263 + seed * 1442695041) & 0xFFFFFFFF
    n = (n ^ (n >> 13)) * 1274126177 & 0xFFFFFFFF
    n = n ^ (n >> 16)
    return (n & 0xFFFF) / 65535.0


def build_scales_textures(tex_dir: Path, size: int = 1024):
    """Tileable 16x16 voxel-cell armored scale pattern (1 cell = 64x64 px -> 16x16 texels)."""
    albedo = bytearray(size * size * 4)
    heights = [0.5] * (size * size)
    cells = 16
    cell_px = size // cells  # 64 px per voxel cell

    for y in range(size):
        cy, ly = divmod(y, cell_px)
        # Pixel-quantized local coordinate inside 16x16 sub-grid per voxel
        py = (ly * 16) // cell_px
        for x in range(size):
            # Brick offset every other row for scale overlap
            cx_raw = x + (cell_px // 2 if (cy % 2 == 1) else 0)
            cx = (cx_raw % size) // cell_px
            lx = cx_raw % cell_px
            px = (lx * 16) // cell_px

            jitter = (hash2d(cx, cy, 42) - 0.5) * 0.08
            # Crisp voxel bevel frame around each scale plate
            border = (px in (0, 15)) or (py in (0, 15))
            inner_rim = (px in (1, 14)) or (py in (1, 14))
            ridge = (px in (7, 8)) and (2 <= py <= 13)

            if border:
                lum = 0.72 + jitter * 0.5
                h = 0.25
            elif inner_rim:
                lum = 0.94 + jitter
                h = 0.78
            elif ridge:
                lum = 0.98 + jitter
                h = 0.92
            else:
                # Subtle micro-voxel grain
                micro = (hash2d(cx * 16 + px // 2, cy * 16 + py // 2, 99) - 0.5) * 0.05
                lum = 0.88 + jitter + micro
                h = 0.65 + micro * 0.6

            lum = max(0.66, min(1.0, lum))
            v = int(lum * 255.0 + 0.5)
            idx = (y * size + x) * 4
            albedo[idx] = v
            albedo[idx + 1] = v
            albedo[idx + 2] = v
            albedo[idx + 3] = 255
            heights[y * size + x] = h

    write_png_rgba(tex_dir / "wyvern_scales_albedo.png", size, size, albedo)
    height_to_normal_png(tex_dir / "wyvern_scales_normal.png", size, size, heights, strength=2.4)


def build_horns_textures(tex_dir: Path, size: int = 1024):
    """Striated keratin/bone voxel texture with longitudinal growth grooves."""
    albedo = bytearray(size * size * 4)
    heights = [0.5] * (size * size)
    pix_grid = 64  # 16 px per voxel step on 1024

    for y in range(size):
        gy = y // 16
        for x in range(size):
            gx = x // 16
            # Longitudinal bands + subtle transverse growth rings
            stripe = ((gx % 4 == 0) * -0.07) + ((gx % 4 == 2) * 0.05)
            ring = -0.06 if (gy % 8 == 0) else 0.0
            grain = (hash2d(gx, gy // 2, 701) - 0.5) * 0.07
            lum = max(0.70, min(1.0, 0.90 + stripe + ring + grain))
            h = max(0.15, min(0.95, 0.62 + stripe * 2.2 + ring * 2.0 + grain))
            v = int(lum * 255.0 + 0.5)
            idx = (y * size + x) * 4
            albedo[idx] = v
            albedo[idx + 1] = v
            albedo[idx + 2] = v
            albedo[idx + 3] = 255
            heights[y * size + x] = h

    write_png_rgba(tex_dir / "wyvern_horns_albedo.png", size, size, albedo)
    height_to_normal_png(tex_dir / "wyvern_horns_normal.png", size, size, heights, strength=1.8)


def build_membranes_textures(tex_dir: Path, size: int = 1024):
    """Voxel-aligned wing bay membrane texture: u=across bay (0..1), v=pivot->trailing edge (0..1)."""
    albedo = bytearray(size * size * 4)
    heights = [0.5] * (size * size)
    pix = 16  # 64x64 pixel-voxel grid over 1024x1024

    for y in range(size):
        gy = y // pix  # 0 (trailing) .. 63 (pivot) depending on V orientation (PNG top=0 -> v=1, bottom -> v=0)
        v_coord = 1.0 - (gy + 0.5) / 64.0
        for x in range(size):
            gx = x // pix
            u_coord = (gx + 0.5) / 64.0
            # Edge border near u=0, u=1 (finger spars) and v near 1.0 (scalloped trailing edge)
            dist_edge = min(u_coord, 1.0 - u_coord)
            is_spar_seam = dist_edge < 0.08
            is_trailing_hem = v_coord > 0.89
            # Pixel-stepped radial veins & transverse leather bands
            vein_pos = abs(((u_coord * 6.0) % 1.0) - 0.5)
            is_radial_vein = vein_pos < 0.07 and v_coord > 0.15
            is_cross_band = (gy % 10 == 0) and not is_spar_seam
            noise = (hash2d(gx, gy, 313) - 0.5) * 0.06

            if is_trailing_hem:
                lum = 0.68 + noise
                h = 0.30
            elif is_spar_seam:
                lum = 0.74 + noise
                h = 0.82
            elif is_radial_vein:
                lum = 0.80 + noise
                h = 0.72
            elif is_cross_band:
                lum = 0.84 + noise
                h = 0.62
            else:
                # Subtle translucent center glow
                center_glow = 0.08 * math.sin(math.pi * u_coord) * (0.4 + 0.6 * (1.0 - abs(v_coord - 0.5)))
                lum = 0.90 + center_glow + noise
                h = 0.45 + noise * 0.5

            lum = max(0.64, min(1.0, lum))
            val = int(lum * 255.0 + 0.5)
            idx = (y * size + x) * 4
            albedo[idx] = val
            albedo[idx + 1] = val
            albedo[idx + 2] = val
            albedo[idx + 3] = 255
            heights[y * size + x] = h

    write_png_rgba(tex_dir / "wyvern_membranes_albedo.png", size, size, albedo)
    height_to_normal_png(tex_dir / "wyvern_membranes_normal.png", size, size, heights, strength=2.0)


def build_eyes_texture(tex_dir: Path, size: int = 512):
    """Crisp reptilian vertical slit pupil + bright neutral iris for multi-palette tinting."""
    albedo = bytearray(size * size * 4)
    grid = 32  # 16x16 px per voxel cell -> sharp pixel-art eye
    cell = size // grid
    for y in range(size):
        gy = y // cell
        ny = (gy + 0.5) / grid * 2.0 - 1.0
        for x in range(size):
            gx = x // cell
            nx = (gx + 0.5) / grid * 2.0 - 1.0
            # Vertical slit pupil in center
            if abs(nx) < 0.19 and abs(ny) < 0.72:
                lum = 0.08
            # Dark outer socket border
            elif max(abs(nx), abs(ny)) > 0.86:
                lum = 0.22
            # Upper specular highlight voxel
            elif 0.16 <= nx <= 0.44 and -0.58 <= ny <= -0.28:
                lum = 1.00
            else:
                # Radiant iris ring
                r = math.hypot(nx * 1.15, ny * 0.85)
                lum = 0.96 if r < 0.72 else 0.78
            val = int(lum * 255.0 + 0.5)
            idx = (y * size + x) * 4
            albedo[idx] = val
            albedo[idx + 1] = val
            albedo[idx + 2] = val
            albedo[idx + 3] = 255
    write_png_rgba(tex_dir / "wyvern_eyes_albedo.png", size, size, albedo)


def generate_all(tex_dir: Path = DEFAULT_TEX_DIR):
    tex_dir.mkdir(parents=True, exist_ok=True)
    build_scales_textures(tex_dir, 1024)
    build_horns_textures(tex_dir, 1024)
    build_membranes_textures(tex_dir, 1024)
    build_eyes_texture(tex_dir, 512)
    return sorted(str(p.name) for p in tex_dir.glob("*.png"))


if __name__ == "__main__":
    files = generate_all()
    print("Generated textures:", files)
