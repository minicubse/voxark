"""Exercise the generic creature validator through its command-line interface."""

import copy
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest
import zlib


REPO = Path(__file__).resolve().parents[2]
VALIDATOR = REPO / "tools/validate-creature.py"


def png(width, height):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(bytes(height * (1 + width * 3))))
        + chunk(b"IEND", b"")
    )


def fixture():
    manifest = json.loads((REPO / "assets/creatures/wyvern.json").read_text(encoding="utf-8"))
    manifest["model"] = "creatures/alpine.gltf"
    manifest["scene"] = "Alpine takes"
    manifest["scale"] = 0.65
    manifest["clips"] = {key: f"Take {index + 1}" for index, key in enumerate(manifest["clips"])}
    manifest["slots"] = {key: f"Material {index + 1}" for index, key in enumerate(manifest["slots"])}
    data = bytearray()
    document = {
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"name": manifest["scene"], "nodes": [0, 3]}],
        "nodes": [
            {"name": "Motion reference", "children": [1]},
            {"name": "Pelvis", "children": [2]},
            {"name": "Fin", "translation": [0, 0.3, 0]},
            {"mesh": 0, "skin": 0},
        ],
        "skins": [{"skeleton": 0, "joints": [1, 2]}],
        "materials": [{"name": name} for name in manifest["slots"].values()],
        "meshes": [{"primitives": []}],
        "animations": [],
        "accessors": [],
        "bufferViews": [],
    }

    def accessor(format_code, component, shape, rows, **properties):
        data.extend(bytes(-len(data) % 4))
        offset = len(data)
        for row in rows:
            data.extend(struct.pack("<" + format_code * len(row), *row))
        view = len(document["bufferViews"])
        document["bufferViews"].append({"buffer": 0, "byteOffset": offset, "byteLength": len(data) - offset})
        index = len(document["accessors"])
        document["accessors"].append({
            "bufferView": view, "componentType": component, "type": shape, "count": len(rows), **properties,
        })
        return index

    identity = (1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)
    inverse_fin = (*identity[:13], -0.3, *identity[14:])
    document["skins"][0]["inverseBindMatrices"] = accessor("f", 5126, "MAT4", [identity, inverse_fin])
    attrs = {
        "POSITION": accessor("f", 5126, "VEC3", [(0, 0, 0), (1, 0, 0), (0, 0.5, 0)], min=[0, 0, 0], max=[1, 0.5, 0]),
        "TEXCOORD_0": accessor("f", 5126, "VEC2", [(0, 0), (1, 0), (0, 1)]),
        "JOINTS_0": accessor("H", 5123, "VEC4", [(0, 1, 0, 0)] * 3),
        "WEIGHTS_0": accessor("B", 5121, "VEC4", [(127, 128, 0, 0)] * 3, normalized=True),
    }
    indices = accessor("H", 5123, "SCALAR", [(0,), (1,), (2,)])
    for material in range(len(document["materials"])):
        document["meshes"][0]["primitives"].append({"attributes": dict(attrs), "indices": indices, "material": material})
    times = accessor("f", 5126, "SCALAR", [(0,), (1.25,)], min=[0], max=[1.25])
    fin_translation = accessor("f", 5126, "VEC3", [(0, 0.3, 0), (0.2, 0.3, 0)])
    constant_root = accessor("f", 5126, "VEC3", [(0, 0, 0), (0, 0, 0)])
    for name in manifest["clips"].values():
        document["animations"].append({
            "name": name,
            "samplers": [{"input": times, "output": fin_translation}, {"input": times, "output": constant_root}],
            "channels": [
                {"sampler": 0, "target": {"node": 2, "path": "translation"}},
                {"sampler": 1, "target": {"node": 0, "path": "translation"}},
            ],
        })
    document["images"] = [{"uri": "skin%20paint.png"}]
    document["textures"] = [{"source": 0}]
    document["materials"][0]["pbrMetallicRoughness"] = {"baseColorTexture": {"index": 0}}
    document["buffers"] = [{"uri": "mesh%20data.bin", "byteLength": len(data)}]
    return manifest, document, data


class ValidatorTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name) / "assets"
        self.directory = self.root / "creatures"
        self.directory.mkdir(parents=True)
        self.manifest_path = self.directory / "alpine.json"
        self.manifest, self.document, self.data = fixture()
        (self.directory / "skin paint.png").write_bytes(png(8, 4))

    def run_validator(self, *options, manifest_path=None):
        result = subprocess.run([
            sys.executable, VALIDATOR, "--manifest", manifest_path or self.manifest_path, *options,
        ], cwd=self.root.parent, capture_output=True, text=True, encoding="utf-8", check=False)
        self.assertIn(result.returncode, (0, 1), result.stderr)
        self.assertEqual(result.stderr, "")
        report = json.loads(result.stdout)
        self.assertEqual(result.returncode, int(not report["valid"]), report)
        return report

    def write_fixture(self):
        self.manifest_path.write_text(json.dumps(self.manifest), encoding="utf-8")
        (self.directory / "alpine.gltf").write_text(json.dumps(self.document), encoding="utf-8")
        (self.directory / "mesh data.bin").write_bytes(self.data)

    def reject(self, message):
        self.write_fixture()
        report = self.run_validator("--asset-root", self.root)
        self.assertFalse(report["valid"], report)
        self.assertIn(message.lower(), " ".join(report["errors"]).lower())

    def test_shipped_glb_passes(self):
        report = self.run_validator(manifest_path=REPO / "assets/creatures/wyvern.json")
        self.assertTrue(report["valid"], report)
        self.assertEqual(report["metrics"]["triangles"], 6048)

    def test_arbitrary_model_with_external_buffer_and_texture_passes(self):
        del self.manifest["scene"]  # Omitted uses the glTF's default scene.
        self.write_fixture()
        path = self.root.parent / "reports" / "alpine.json"
        report = self.run_validator("--result", path)
        self.assertTrue(report["valid"], report)
        self.assertEqual(report["metrics"]["triangles"], 4)
        self.assertEqual(report["metrics"]["textures"][0]["width"], 8)
        self.assertEqual(report["metrics"]["clips"]["Walk"]["duration_seconds"], 1.25)
        self.assertTrue(report["manual_checks"])
        self.assertEqual(json.loads(path.read_text(encoding="utf-8")), report)

    def test_glb_with_embedded_texture_passes(self):
        self.manifest["model"] = "creatures/alpine.glb"
        image = png(16, 8)
        self.data.extend(bytes(-len(self.data) % 4))
        self.document["images"][0] = {"bufferView": len(self.document["bufferViews"]), "mimeType": "image/png"}
        self.document["bufferViews"].append({"buffer": 0, "byteOffset": len(self.data), "byteLength": len(image)})
        self.data.extend(image)
        self.document["buffers"][0] = {"byteLength": len(self.data)}
        metadata = json.dumps(self.document).encode("utf-8")
        metadata += b" " * (-len(metadata) % 4)
        binary = self.data + bytes(-len(self.data) % 4)
        glb = struct.pack("<4sII", b"glTF", 2, len(metadata) + len(binary) + 28)
        glb += struct.pack("<I4s", len(metadata), b"JSON") + metadata
        glb += struct.pack("<I4s", len(binary), b"BIN\0") + binary
        (self.directory / "alpine.glb").write_bytes(glb)
        self.write_fixture()
        report = self.run_validator()
        self.assertTrue(report["valid"], report)
        self.assertEqual(report["metrics"]["textures"][0]["height"], 8)

    def test_invalid_glb(self):
        self.manifest["model"] = "creatures/broken.glb"
        (self.directory / "broken.glb").write_bytes(b"not a GLB")
        self.reject("GLB")

    def test_missing_clip(self):
        self.document["animations"].pop()
        self.reject("clip")

    def test_wrong_slot(self):
        self.document["materials"][0]["name"] = "Unknown"
        self.reject("slot")

    def test_triangle_budget(self):
        mesh = self.document["meshes"][0]
        mesh["primitives"] += [copy.deepcopy(mesh["primitives"][0]) for _ in range(18000)]
        self.reject("triangle budget")

    def test_texture_budget(self):
        (self.directory / "skin paint.png").write_bytes(png(2049, 1))
        self.reject("texture budget")

    def test_unweighted_vertex(self):
        attr = self.document["meshes"][0]["primitives"][0]["attributes"]["WEIGHTS_0"]
        view = self.document["accessors"][attr]["bufferView"]
        offset = self.document["bufferViews"][view]["byteOffset"]
        self.data[offset:offset + 4] = b"\x00" * 4
        self.reject("weights")

    def test_out_of_range_joint_index(self):
        attr = self.document["meshes"][0]["primitives"][0]["attributes"]["JOINTS_0"]
        view = self.document["accessors"][attr]["bufferView"]
        offset = self.document["bufferViews"][view]["byteOffset"]
        struct.pack_into("<H", self.data, offset, 2)
        self.reject("joint index")

    def test_more_than_four_influences(self):
        attributes = self.document["meshes"][0]["primitives"][0]["attributes"]
        attributes["JOINTS_1"] = attributes["JOINTS_0"]
        attributes["WEIGHTS_1"] = attributes["WEIGHTS_0"]
        self.reject("four influences")

    def test_singular_bind_matrix(self):
        view = self.document["accessors"][self.document["skins"][0]["inverseBindMatrices"]]["bufferView"]
        offset = self.document["bufferViews"][view]["byteOffset"]
        struct.pack_into("<f", self.data, offset, 0)
        self.reject("bind matrix")

    def test_hierarchy_cycle(self):
        self.document["nodes"][2]["children"] = [0]
        self.reject("cycle")

    def test_buffer_overrun(self):
        self.document["bufferViews"][0]["byteLength"] = len(self.data) + 4
        self.reject("bufferView")

    def test_scene_root_motion(self):
        self.document["animations"][0]["samplers"][1]["output"] = self.document["animations"][0]["samplers"][0]["output"]
        self.reject("root motion")

    def test_nonincreasing_animation_time(self):
        index = self.document["animations"][0]["samplers"][0]["input"]
        view = self.document["accessors"][index]["bufferView"]
        offset = self.document["bufferViews"][view]["byteOffset"]
        struct.pack_into("<f", self.data, offset + 4, 0)
        self.document["accessors"][index]["max"] = [0]
        self.reject("timestamps")

    def test_manifest_path_escape(self):
        self.manifest["model"] = "../alpine.gltf"
        self.reject("relative")

    def test_external_resource_path_escape(self):
        self.document["images"][0]["uri"] = "../../%70aint.png"
        (self.root.parent / "paint.png").write_bytes(png(8, 4))
        self.reject("escapes the asset root")

    def test_missing_image(self):
        self.document["images"][0]["uri"] = "missing.png"
        self.reject("missing.png")

    def test_malformed_manifest_writes_json_report_on_failure(self):
        self.manifest_path.write_text("{", encoding="utf-8")
        path = self.root.parent / "failure.json"
        report = self.run_validator("--result", path)
        self.assertFalse(report["valid"], report)
        self.assertIn("Creature manifest", report["errors"][0])
        self.assertEqual(json.loads(path.read_text(encoding="utf-8")), report)


if __name__ == "__main__":
    unittest.main()
