"""Generate the original Primal Voxel wyvern as a self-contained glTF 2.0 binary."""

import argparse
from collections import defaultdict
import json
import math
from pathlib import Path
import random
import struct


DEFAULT_PATH = Path(__file__).resolve().parents[1] / "assets/creatures/wyvern.glb"
DARK = (0.093, 0.178, 0.152)
SCALE = (0.174, 0.314, 0.255)
RIDGE = (0.212, 0.367, 0.284)
BELLY = (0.342, 0.234, 0.129)
BONE = (0.58, 0.408, 0.224)
BLACK = (0.041, 0.037, 0.031)
RUST = (0.49, 0.203, 0.112)
MEMBRANE = (0.375, 0.117, 0.078)


def add(a, b):
    return tuple(x + y for x, y in zip(a, b, strict=True))


def sub(a, b):
    return tuple(x - y for x, y in zip(a, b, strict=True))


def mul(a, scalar):
    return tuple(x * scalar for x in a)


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def unit(v):
    return mul(v, 1 / math.hypot(*v))


def mix(a, b, t):
    return add(mul(a, 1 - t), mul(b, t))


def weighted(weights, coefficients):
    combined = defaultdict(float)
    for items, coefficient in zip(weights, coefficients, strict=True):
        for joint, weight in items.items():
            combined[joint] += weight * coefficient
    return combined


def rgba(rgb):
    return (*(min(1.0, max(0.0, channel)) for channel in rgb), 1.0)


def pbr(name, roughness, **extras):
    return {"name": name, "pbrMetallicRoughness": {
        "baseColorFactor": [1, 1, 1, 1], "metallicFactor": 0, "roughnessFactor": roughness,
    }, **extras}


class Gltf:
    def __init__(self):
        self.blob = bytearray()
        self.document = {
            "asset": {"version": "2.0", "generator": "Primal Voxel · procedural wyvern"},
            "accessors": [], "bufferViews": [], "nodes": [], "animations": [],
            "materials": [pbr("Scales", 0.77), pbr("Horns", 0.66),
                          pbr("Eyes", 0.34), pbr("Membranes", 0.87, doubleSided=True)],
        }

    def accessor(self, rows, shape, component=5126, target=None, bounded=False):
        while len(self.blob) % 4:
            self.blob.append(0)
        number = {"SCALAR": 1, "VEC3": 3, "VEC4": 4, "MAT4": 16}[shape]
        char = {5123: "H", 5126: "f"}[component]
        layout = struct.Struct("<" + char * number)
        offset = len(self.blob)
        for row in rows:
            self.blob.extend(layout.pack(*row))
        view = {"buffer": 0, "byteOffset": offset, "byteLength": len(self.blob) - offset}
        if target is not None:
            view["target"] = target
        views = self.document["bufferViews"]
        access = {"bufferView": len(views), "componentType": component, "count": len(rows), "type": shape}
        if bounded:
            access["min"] = [min(axis) for axis in zip(*rows, strict=True)]
            access["max"] = [max(axis) for axis in zip(*rows, strict=True)]
        views.append(view)
        self.document["accessors"].append(access)
        return len(self.document["accessors"]) - 1

    def write(self, path):
        self.document["buffers"] = [{"byteLength": len(self.blob)}]
        metadata = json.dumps(self.document, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
        metadata += b" " * (-len(metadata) % 4)
        self.blob.extend(b"\x00" * (-len(self.blob) % 4))
        header = struct.pack("<4sIII4s", b"glTF", 2, 28 + len(metadata) + len(self.blob), len(metadata), b"JSON")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(header + metadata + struct.pack("<I4s", len(self.blob), b"BIN\x00") + self.blob)


class Surface:
    def __init__(self):
        self.positions = []
        self.normals = []
        self.colors = []
        self.joints = []
        self.weights = []
        self.indices = []

    def vertex(self, point, normal, color, weights):
        strongest = sorted(((joint, weight) for joint, weight in weights.items() if weight > 1e-7),
                           key=lambda entry: -entry[1])[:4]
        total = sum(weight for _, weight in strongest)
        strongest.extend([(0, 0)] * (4 - len(strongest)))
        self.positions.append(point)
        self.normals.append(unit(normal))
        self.colors.append(rgba(color))
        self.joints.append(tuple(joint for joint, _ in strongest))
        self.weights.append(tuple(weight / total for _, weight in strongest))
        return len(self.positions) - 1

    def tri(self, a, b, c, flip=False):
        self.indices.extend((a, c, b) if flip else (a, b, c))

    def primitive(self, gltf, material):
        attributes = {
            "POSITION": gltf.accessor(self.positions, "VEC3", target=34962, bounded=True),
            "NORMAL": gltf.accessor(self.normals, "VEC3", target=34962),
            "COLOR_0": gltf.accessor(self.colors, "VEC4", target=34962),
            "JOINTS_0": gltf.accessor(self.joints, "VEC4", 5123, 34962),
            "WEIGHTS_0": gltf.accessor(self.weights, "VEC4", target=34962),
        }
        indices = gltf.accessor([(i,) for i in self.indices], "SCALAR", 5123, 34963)
        return {"attributes": attributes, "indices": indices, "material": material}


class Wyvern:
    def __init__(self):
        self.gltf = Gltf()
        self.surfaces = [Surface() for _ in self.gltf.document["materials"]]
        self.positions = {}
        self.joints = {}
        self.random = random.Random(41807)

    def bone(self, name, parent, point):
        nodes = self.gltf.document["nodes"]
        index = len(nodes)
        translation = point
        if parent is not None:
            nodes[self.joints[parent]].setdefault("children", []).append(index)
            translation = sub(point, self.positions[parent])
        self.positions[name] = point
        self.joints[name] = index
        nodes.append({"name": name, "translation": translation})

    def rig(self):
        for name, parent, point in (
            ("Root", None, (0, 0, 0)),
            ("Body", "Root", (0, 1.18, -0.13)),
            ("NeckBase", "Body", (0, 1.48, 0.62)),
            ("NeckTip", "NeckBase", (0, 1.9, 1.13)),
            ("Head", "NeckTip", (0, 2.23, 1.67)),
            ("Jaw", "Head", (0, 2.03, 1.68)),
            ("TailBase", "Body", (0, 1.19, -0.82)),
            ("TailMid", "TailBase", (0.018, 1.03, -1.72)),
            ("TailTip", "TailMid", (0.06, 0.79, -2.63)),
        ):
            self.bone(name, parent, point)
        for side, sign in (("L", 1), ("R", -1)):
            for name, parent, point in (
                ("Hip", "Body", (0.41, 1.15, -0.54)),
                ("Knee", "Hip", (0.54, 0.65, -0.30)),
                ("Ankle", "Knee", (0.64, 0.25, -0.78)),
                ("WingShoulder", "Body", (0.43, 1.49, 0.49)),
                ("WingElbow", "WingShoulder", (1.19, 1.64, 1.07)),
                ("WingWrist", "WingElbow", (2.03, 1.78, 0.91)),
                ("WingDigit", "WingWrist", (2.77, 1.56, 0.36)),
            ):
                self.bone(f"{name}.{side}", parent if parent == "Body" else f"{parent}.{side}",
                          (point[0] * sign, point[1], point[2]))

    def weights(self, mapping):
        if isinstance(mapping, str):
            return {self.joints[mapping]: 1.0}
        return {self.joints[name]: amount for name, amount in mapping.items()}

    def tube(self, material, rings, sides=10, hint=(0, 1, 0), underside=None, mottled=0.10):
        """Loft elliptical cross sections along an anatomical path."""
        surface = self.surfaces[material]
        start = len(surface.positions)
        for index, (point, width, height, joint, color) in enumerate(rings):
            tangent = unit(sub(rings[min(index + 1, len(rings) - 1)][0], rings[max(0, index - 1)][0]))
            u = unit(cross(tangent, hint))
            v = unit(cross(tangent, u))
            if material == 0:
                mottling_phase = self.random.random() * math.tau
            influence = self.weights(joint)
            for spoke in range(sides):
                angle = spoke * math.tau / sides
                radial = add(mul(u, math.cos(angle) * width), mul(v, math.sin(angle) * height))
                slope = add(mul(u, math.cos(angle) / width), mul(v, math.sin(angle) / height))
                shade = color
                if underside is not None:
                    shade = mix(color, underside, max(0.0, -unit(radial)[1]) ** 3)
                noise = self.random.uniform(-mottled, mottled)
                if material == 0:
                    noise += 0.035 * math.cos(3 * angle + mottling_phase)
                surface.vertex(add(point, radial), slope, mul(shade, 1 + noise), influence)
        for index in range(len(rings) - 1):
            for spoke in range(sides):
                a = start + sides * index + spoke
                b = start + sides * index + (spoke + 1) % sides
                surface.tri(a, b, a + sides)
                surface.tri(b, b + sides, a + sides)
        for first in (True, False):
            index = 0 if first else len(rings) - 1
            point, _, _, joint, color = rings[index]
            direction = sub(point, rings[1 if first else -2][0])
            base = start + index * sides
            cap = surface.vertex(point, direction, color, self.weights(joint))
            for spoke in range(sides):
                surface.tri(cap, base + spoke, base + (spoke + 1) % sides, first)

    def taper(self, material, points, radii, joint, color, tip=None, sides=7, hint=(0, 1, 0)):
        last = len(points) - 1
        start_color = color
        end_color = tip if tip is not None else color
        self.tube(material, [(point, width, width * 0.84, joint, mix(start_color, end_color, i / last))
                             for i, (point, width) in enumerate(zip(points, radii, strict=True))], sides, hint)

    def torso(self):
        self.tube(0, [
            ((0, 1.11, -1.0), 0.19, 0.22, "TailBase", DARK),
            ((0, 1.18, -0.74), 0.44, 0.38, {"Body": .72, "TailBase": .28}, DARK),
            ((0, 1.23, -0.34), 0.59, 0.51, "Body", SCALE),
            ((0, 1.31, 0.18), 0.64, 0.56, "Body", RIDGE),
            ((0, 1.39, 0.57), 0.48, 0.46, "Body", SCALE),
            ((0, 1.47, 0.82), 0.28, 0.30, {"Body": .51, "NeckBase": .49}, DARK),
        ], 16, underside=BELLY)
        self.tube(0, [
            ((0, 1.40, 0.45), 0.41, 0.39, "Body", DARK),
            ((0, 1.55, 0.72), 0.36, 0.36, {"Body": .35, "NeckBase": .65}, RIDGE),
            ((0, 1.76, 0.96), 0.30, 0.30, {"NeckBase": .62, "NeckTip": .38}, SCALE),
            ((0, 1.99, 1.23), 0.245, 0.265, {"NeckTip": .82, "Head": .18}, SCALE),
            ((0, 2.19, 1.61), 0.235, 0.25, {"NeckTip": .37, "Head": .63}, DARK),
            ((0, 2.23, 1.84), 0.27, 0.25, "Head", DARK),
        ], 14, underside=BELLY)
        self.tube(0, [
            ((0, 1.22, -0.61), 0.31, 0.31, {"Body": .53, "TailBase": .47}, RIDGE),
            ((0, 1.16, -1.08), 0.28, 0.27, "TailBase", SCALE),
            ((0.02, 1.04, -1.65), 0.20, 0.22, {"TailBase": .43, "TailMid": .57}, DARK),
            ((0.08, 0.89, -2.16), 0.14, 0.16, "TailMid", SCALE),
            ((0.07, 0.79, -2.65), 0.093, 0.107, {"TailMid": .31, "TailTip": .69}, DARK),
            ((0.005, 0.83, -3.13), 0.063, 0.07, "TailTip", RIDGE),
            ((-0.12, 0.96, -3.52), 0.034, 0.055, "TailTip", SCALE),
            ((-0.26, 1.05, -3.73), 0.006, 0.012, "TailTip", DARK),
        ], 12, underside=BELLY)

        crest = [
            ((0, 2.37, 1.31), .22, "NeckTip"), ((0, 2.14, 1.00), .26, "NeckBase"),
            ((0, 1.92, 0.65), .34, "Body"), ((0, 1.86, 0.26), .36, "Body"),
            ((0, 1.81, -0.13), .35, "Body"), ((0, 1.71, -0.51), .31, "Body"),
            ((0, 1.49, -0.96), .23, "TailBase"), ((0.01, 1.38, -1.41), .19, "TailBase"),
            ((0.03, 1.24, -1.79), .17, "TailMid"), ((0.08, 1.075, -2.20), .12, "TailMid"),
            ((0.06, 0.91, -2.61), .10, "TailTip"), ((0.01, 0.91, -3.04), .085, "TailTip"),
        ]
        for point, height, joint in crest:
            self.taper(1, [point, add(point, (0, height * .73, -height * .31)),
                           add(point, (height * .14, height, -height * .86))],
                       [height * .32, height * .20, .007], joint, DARK, BONE, hint=(1, 0, 0))
        for x_sign in (-1, 1):
            for index in range(7):
                y = 1.75 + index * .052
                z = .46 + index * .146
                x = x_sign * (.36 - index * .023)
                joint = "NeckBase" if index < 3 else "NeckTip"
                self.taper(0, [(x * .95, y, z + .08), (x * 1.04, y, z),
                               (x * 1.08, y + .055, z - .09)],
                           [.042, .046, .016], joint, RIDGE, BELLY)

    def head(self):
        self.tube(0, [
            ((0, 2.21, 1.49), .19, .21, "Head", DARK),
            ((0, 2.27, 1.67), .31, .26, "Head", RIDGE),
            ((0, 2.29, 1.88), .34, .24, "Head", SCALE),
            ((0, 2.235, 2.12), .25, .178, "Head", SCALE),
            ((0, 2.19, 2.41), .211, .126, "Head", DARK),
            ((0, 2.21, 2.58), .14, .099, "Head", DARK),
            ((0, 2.22, 2.64), .07, .05, "Head", BLACK),
        ], 14, underside=mul(BELLY, .72))
        self.tube(0, [
            ((0, 2.035, 1.59), .21, .115, "Jaw", DARK),
            ((0, 2.005, 1.85), .285, .122, "Jaw", BELLY),
            ((0, 2.005, 2.14), .209, .083, "Jaw", RUST),
            ((0, 2.052, 2.42), .174, .066, "Jaw", BELLY),
            ((0, 2.083, 2.56), .101, .04, "Jaw", DARK),
        ], 12)
        for sign in (1, -1):
            def p(x, y, z):
                return (sign * x, y, z)

            self.taper(0, [p(.23, 2.16, 1.59), p(.32, 2.135, 1.82),
                           p(.22, 2.11, 2.20), p(.16, 2.12, 2.5)],
                       [.09, .125, .052, .022], "Head", DARK, SCALE)
            self.taper(0, [p(.25, 2.35, 2.10), p(.306, 2.435, 1.91),
                           p(.29, 2.43, 1.71), p(.28, 2.39, 1.57)],
                       [.042, .083, .087, .022], "Head", DARK, RIDGE)
            self.taper(0, [p(.293, 2.32, 1.76), p(.338, 2.323, 1.888),
                           p(.287, 2.324, 1.999)], [.045, .071, .027], "Head", BLACK)
            self.taper(2, [p(.326, 2.328, 1.83), p(.358, 2.333, 1.907), p(.322, 2.337, 1.972)],
                       [.024, .049, .019], "Head", (.95, .55, .105), (.69, .26, .028))
            self.taper(2, [p(.362, 2.292, 1.917), p(.373, 2.335, 1.917), p(.365, 2.378, 1.911)],
                       [.006, .012, .006], "Head", BLACK, hint=(1, 0, 0))
            self.taper(2, [p(.363, 2.354, 1.893), p(.367, 2.364, 1.913)],
                       [.009, .006], "Head", (.92, .83, .67))
            self.taper(0, [p(.183, 2.238, 2.407), p(.199, 2.247, 2.462), p(.158, 2.249, 2.516)],
                       [.024, .047, .020], "Head", BLACK, hint=(1, 0, 0))
            self.taper(1, [p(.245, 2.414, 1.675), p(.351, 2.581, 1.535),
                           p(.463, 2.726, 1.276), p(.515, 2.793, 1.014 if sign > 0 else 1.12)],
                       [.143, .107, .066, .009 if sign > 0 else .035], "Head", DARK, BONE)
            self.taper(1, [p(.254, 2.262, 1.652), p(.381, 2.299, 1.448),
                           p(.481, 2.324, 1.179)], [.086, .066, .009], "Head", BLACK, BONE)
            self.taper(1, [p(.218, 2.15, 1.72), p(.38, 2.127, 1.529), p(.42, 2.146, 1.345)],
                       [.075, .044, .010], "Head", DARK, RUST)
            for i in range(4):
                z = 2.16 + i * .104
                x = .189 - i * .012
                self.taper(1, [p(x, 2.086 + i * .015, z), p(x, 2.017 + i * .016, z + .035)],
                           [.026 if i == 2 else .018, .004], "Head", BONE, (.75, .64, .42),
                           hint=(1, 0, 0))
        # A three-part scar and chipped horn distinguish the right side.
        for index in range(3):
            self.taper(0, [(-.322 + index * .018, 2.236, 1.91 + index * .09),
                           (-.333 + index * .022, 2.201, 1.878 + index * .09),
                           (-.319 + index * .02, 2.168, 1.851 + index * .09)],
                       [.009, .013, .007], "Head", RUST, BELLY, sides=5)

    def leg(self, side, sign):
        hip, knee, ankle = (f"{joint}.{side}" for joint in ("Hip", "Knee", "Ankle"))
        h, k, a = (self.positions[joint] for joint in (hip, knee, ankle))
        self.tube(0, [
            (add(h, (-.14 * sign, .17, -.07)), .22, .24, "Body", DARK),
            (add(h, (.03 * sign, -.03, 0)), .316, .336, hip, RIDGE),
            (mix(h, k, .51), .278, .241, hip, SCALE),
            (k, .184, .184, {hip: .49, knee: .51}, SCALE),
            (mix(k, a, .58), .127, .137, knee, DARK),
            (a, .104, .101, {knee: .30, ankle: .70}, SCALE),
            (add(a, (0, -.125, .16)), .144, .082, ankle, BELLY),
            (add(a, (0, -.128, .30)), .104, .065, ankle, DARK),
        ], 11, hint=(1, 0, 0), underside=BELLY)
        for i in (-1, 0, 1):
            toe_root = add(a, (i * .079, -.138, .20))
            knuckle = add(a, (i * .154, -.167, .36 + (i == 0) * .05))
            toe_tip = add(knuckle, (i * .019, -.018, .13))
            self.taper(0, [toe_root, knuckle, toe_tip], [.057, .054, .033], ankle, SCALE, DARK)
            self.taper(1, [toe_tip, add(toe_tip, (0, .015, .075)), add(toe_tip, (0, -.026, .134))],
                       [.035, .023, .004], ankle, BLACK, BONE)
        self.taper(1, [add(a, (.07 * sign, -.06, .08)), add(a, (.14 * sign, -.10, -.021)),
                       add(a, (.17 * sign, -.147, -.10))], [.049, .035, .008], ankle, DARK, BONE)

    def membrane(self, pivot, start, end, bound, sign, bay):
        surface = self.surfaces[3]
        segments = 9
        grid = {}
        anchors = [point for point, _ in (pivot, start, end)]
        anchor_weights = [self.weights(joint) for _, joint in (pivot, start, end)]
        normal = unit(cross(sub(anchors[1], anchors[0]), sub(anchors[2], anchors[0])))
        flip = normal[1] < 0
        for row in range(segments + 1):
            for column in range(segments - row + 1):
                b, c = row / segments, column / segments
                a = 1 - b - c
                radial = b + c
                along = c / radial if radial else 0
                scallop = math.sin(math.pi * along) * radial**3 * bound
                bag = math.sin(math.pi * radial) * math.sin(math.pi * along) * .105
                point = add(add(mul(anchors[0], a), mul(anchors[1], b)), mul(anchors[2], c))
                point = add(point, (0, -bag, scallop))
                shade = .71 + .29 * math.sin(math.pi * along) + .14 * math.sin(math.pi * radial)
                color = mul(mix(MEMBRANE, RUST, .32 * math.sin(math.pi * radial)), shade)
                veins = math.cos(along * 31 + radial * 4 + bay)
                color = mul(color, .87 + .13 * radial + .07 * veins)
                if sign < 0 and bay == 1 and .42 < along < .68 and radial > .52:
                    color = mix(color, (.58, .285, .162), .25)
                grid[row, column] = surface.vertex(point, mul(normal, -1) if flip else normal, color,
                                                   weighted(anchor_weights, (a, b, c)))
        for row in range(segments):
            for column in range(segments - row):
                a = grid[row, column]
                b = grid[row + 1, column]
                c = grid[row, column + 1]
                surface.tri(a, b, c, flip)
                if row + column < segments - 1:
                    surface.tri(c, b, grid[row + 1, column + 1], flip)

    def wing(self, side, sign):
        shoulder, elbow, wrist, digit = (f"Wing{joint}.{side}" for joint in ("Shoulder", "Elbow", "Wrist", "Digit"))
        s, e, w, d = (self.positions[joint] for joint in (shoulder, elbow, wrist, digit))
        def p(x, y, z):
            return (x * sign, y, z)

        tip = p(3.68 if sign > 0 else 3.62, 1.23, -.38)
        self.tube(0, [
            (p(.29, 1.43, .39), .25, .22, "Body", DARK),
            (s, .245, .185, {shoulder: .72, "Body": .28}, SCALE),
            (mix(s, e, .48), .176, .156, shoulder, RIDGE),
            (e, .127, .118, {shoulder: .46, elbow: .54}, DARK),
            (mix(e, w, .51), .086, .091, elbow, SCALE),
            (w, .103, .11, {elbow: .39, wrist: .61}, DARK),
            (mix(w, d, .49), .067, .056, wrist, SCALE),
            (d, .048, .046, {wrist: .27, digit: .73}, DARK),
            (mix(d, tip, .56), .024, .025, digit, RUST),
            (tip, .007, .008, digit, DARK),
        ], 11)
        self.taper(1, [w, p(2.068, 1.849, 1.074), p(2.015, 1.87, 1.259),
                       p(1.91, 1.803, 1.284)], [.093, .071, .031, .008], wrist, DARK, BONE)
        tips = [
            (tip, digit),
            (p(3.18, 1.19, -1.27), {digit: .75, wrist: .25}),
            (p(2.52, 1.114, -1.71), {digit: .37, wrist: .63}),
            (p(1.62, 1.078, -1.68), {wrist: .38, elbow: .46, shoulder: .16}),
            (p(.433, 1.13, -.75), {"Body": .86, shoulder: .14}),
        ]
        for end, influence in tips[1:-1]:
            mid = mix(w, end, .58)
            self.tube(0, [
                (w, .035, .026, wrist, DARK),
                (mid, .022, .018, weighted(({wrist: 1}, influence), (.42, .58)), RUST),
                (end, .006, .009, influence, DARK),
            ], 7)
        self.membrane((w, wrist), (e, elbow), (s, shoulder), 0, sign, 4)
        self.membrane((w, wrist), (s, shoulder), tips[-1], 0, sign, 5)
        knuckle = (d, {wrist: .27, digit: .73})
        self.membrane((w, wrist), knuckle, tips[1], 0, sign, 6)
        self.membrane(knuckle, tips[0], tips[1], .285, sign, 0)
        for index in range(1, len(tips) - 1):
            self.membrane((w, wrist), tips[index], tips[index + 1], .285 + .035 * index, sign, index)

    def mesh(self):
        self.rig()
        self.torso()
        self.head()
        for side, sign in (("L", 1), ("R", -1)):
            self.leg(side, sign)
            self.wing(side, sign)
        document = self.gltf.document
        document["meshes"] = [{"name": "Wyvern", "primitives": [
            surface.primitive(self.gltf, material) for material, surface in enumerate(self.surfaces)
        ]}]
        matrices = [(1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, -x, -y, -z, 1)
                    for x, y, z in self.positions.values()]
        document["skins"] = [{"name": "Wyvern Rig", "inverseBindMatrices": self.gltf.accessor(matrices, "MAT4"),
                              "skeleton": 0, "joints": list(self.joints.values())}]
        document["scenes"] = [{"name": "Wyvern", "nodes": [0, len(document["nodes"])]}]
        document["scene"] = 0
        document["nodes"].append({"name": "Wyvern Mesh", "mesh": 0, "skin": 0})


def quaternion(x, y, z):
    """Compose local Euler X, then Y, then Z into a glTF quaternion."""
    sx, sy, sz = (math.sin(angle / 2) for angle in (x, y, z))
    cx, cy, cz = (math.cos(angle / 2) for angle in (x, y, z))
    return (sx * cy * cz - cx * sy * sz, cx * sy * cz + sx * cy * sz,
            cx * cy * sz - sx * sy * cz, cx * cy * cz + sx * sy * sz)


def wing_pose(fold, lift, drive, flex):
    rotations = {}
    for side, sign in (("L", 1), ("R", -1)):
        phase = (1 if sign > 0 else -1) * .022
        rotations[f"WingShoulder.{side}"] = (.09 * drive, sign * (.88 * fold + drive * .18),
                                           sign * (lift + .20 * fold + phase))
        rotations[f"WingElbow.{side}"] = (.06 * flex, sign * (.30 * fold - drive * .15),
                                        sign * (.14 * fold - flex * .22))
        rotations[f"WingWrist.{side}"] = (-.09 * flex, sign * (.28 * fold - drive * .18),
                                        sign * (.05 * fold - flex * .45))
        rotations[f"WingDigit.{side}"] = (.14 * flex, sign * (.24 * fold - drive * .12),
                                        sign * (-.14 * fold - flex * .35))
    return rotations


def pose(bob, chest, neck, head, jaw, tail, hips, knees, ankles, wing):
    rotations = {
        "Body": (chest, 0, .022 * tail),
        "NeckBase": (neck, .07 * tail, 0),
        "NeckTip": (-.65 * neck, -.04 * tail, 0),
        "Head": (head, .055 * tail, -.032 * tail),
        "Jaw": (jaw, 0, 0),
        "TailBase": (-.10 * chest, .22 * tail, 0),
        "TailMid": (.12 * chest, .31 * tail, .027 * tail),
        "TailTip": (.21 * chest, .42 * tail, -.03 * tail),
    }
    for index, side in enumerate(("L", "R")):
        rotations[f"Hip.{side}"] = (hips[index], 0, 0)
        rotations[f"Knee.{side}"] = (knees[index], 0, 0)
        rotations[f"Ankle.{side}"] = (ankles[index], 0, 0)
    return bob, {**rotations, **wing_pose(*wing)}


def animate(wyvern):
    neutral = (0, 0)
    idle_pose = pose(0, 0, 0, 0, 0, 0, neutral, neutral, neutral, (1, 0, 0, 0))
    flap_pose = pose(0, -.06, -.06, .06, .02, 0, (-.3, -.3), (.48, .48),
                     (-.22, -.22), (0, .60, -.09, -.42))
    glide_pose = pose(0, .035, -.08, .065, .03, 0, (-.30, -.30), (.4, .4),
                      (-.18, -.18), (0, .08, .04, -.09))
    idle, walk, flap, glide = [], [], [], []
    for i in range(17):
        u = i / 16
        angle = math.tau * u
        stride = math.sin(angle)
        wave = math.cos(angle)
        idle.append((3.2 * u, pose(.007 * (1 - wave), .015 * stride, -.045 * stride, .09 * stride,
                                   .12 * max(0, -wave), .38 * stride, neutral, neutral, neutral,
                                   (1, .042 * stride, .07 * stride, .09 * stride))))
        swing = (.43 * stride, -.43 * stride)
        knees = (.43 * max(0, wave), .43 * max(0, -wave))
        ankles = (-.32 * max(0, wave), -.32 * max(0, -wave))
        walk.append((1.04 * u, pose(.02 + .045 * abs(stride), .055 * wave, -.075 * wave, .085 * wave,
                                    .04 * abs(stride), .54 * stride, swing, knees, ankles,
                                    (.93, .075 * wave, .12 * stride, .11 * wave))))
        flap.append((1.02 * u, pose(.11 * stride, -.06 + .11 * stride, -.06 - .085 * stride,
                                    .06 + .12 * stride, .02 + .14 * max(0, -stride), .28 * stride,
                                    (-.30, -.3), (.48, .48), (-.22, -.22),
                                    (0, .61 * wave - .27 * stride, -.09 + .21 * stride,
                                     -.42 * wave + .67 * stride))))
        glide.append((3.2 * u, pose(.05 * stride, .035 + .05 * stride, -.08 - .05 * stride,
                                    .065 + .04 * stride, .03 + .06 * max(0, stride), .34 * stride,
                                    (-.30, -.30), (.4, .4), (-.18, -.18),
                                    (0, .08 + .08 * stride, .04 + .09 * stride, -.09 + .10 * stride))))
    flap[0], flap[-1] = (0, flap_pose), (1.02, flap_pose)
    takeoff = [
        (0, idle_pose),
        (.22, pose(-.17, .11, -.18, .20, .33, .15, (-.13, -.16), (.30, .31), (-.15, -.15),
                   (.70, .47, -.11, -.25))),
        (.44, pose(.15, -.21, -.15, .19, .38, -.12, (.25, .30), (-.13, -.12), (.17, .14),
                   (.20, .83, -.04, -.51))),
        (.67, pose(.17, -.24, .02, .14, .25, -.20, (-.34, -.37), (.23, .24), (-.26, -.28),
                   (0, -.24, .22, .45))),
        (.89, pose(.04, -.10, -.03, .06, .10, .08, (-.35, -.32), (.45, .48), (-.24, -.22),
                   (0, .22, -.03, .08))),
        (1.05, flap_pose),
    ]
    land = [
        (0, glide_pose),
        (.24, pose(.08, -.19, .06, .10, .16, -.16, (.30, .24), (.09, .11), (.15, .17),
                   (.01, .44, -.35, -.40))),
        (.47, pose(.09, -.13, .07, -.12, .24, .04, (.10, .09), (.19, .20), (.12, .14),
                   (.17, -.22, -.14, .43))),
        (.68, pose(-.16, .12, -.12, .12, .20, .19, (-.11, -.13), (.28, .29), (-.08, -.09),
                   (.48, .36, .13, -.27))),
        (.94, pose(-.04, .042, -.052, .038, .08, .1, neutral, (.07, .07), (-.05, -.05),
                   (.84, .11, .09, .10))),
        (1.10, idle_pose),
    ]
    for name, keys in (("IdleGround", idle), ("Walk", walk), ("Takeoff", takeoff), ("Flap", flap),
                       ("Glide", glide), ("Land", land)):
        gltf = wyvern.gltf
        time = gltf.accessor([(key[0],) for key in keys], "SCALAR", bounded=True)
        clip = {"name": name, "channels": [], "samplers": []}
        track_names = keys[0][1][1]
        for joint in track_names:
            rotations = [quaternion(*key[1][1][joint]) for key in keys]
            append_track(gltf, clip, wyvern.joints[joint], "rotation", time, rotations, "VEC4")
        translation = [add(wyvern.positions["Body"], (0, key[1][0], 0)) for key in keys]
        append_track(gltf, clip, wyvern.joints["Body"], "translation", time, translation, "VEC3")
        gltf.document["animations"].append(clip)


def append_track(gltf, clip, node, property_name, time, samples, shape):
    clip["channels"].append({"sampler": len(clip["samplers"]), "target": {"node": node, "path": property_name}})
    clip["samplers"].append({"input": time, "output": gltf.accessor(samples, shape), "interpolation": "LINEAR"})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", nargs="?", type=Path, default=DEFAULT_PATH)
    args = parser.parse_args()
    wyvern = Wyvern()
    wyvern.mesh()
    animate(wyvern)
    wyvern.gltf.write(args.path)
    print(f"Wrote {args.path} ({args.path.stat().st_size:,} bytes)")


if __name__ == "__main__":
    main()
