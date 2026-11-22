"""Check the shipped GLB independently of its procedural generator."""

import argparse
import json
import math
from pathlib import Path
import struct


DEFAULT_PATH = Path(__file__).resolve().parents[1] / "assets/creatures/wyvern.glb"
CLIPS = {"IdleGround", "Walk", "Takeoff", "Flap", "Glide", "Land"}
MATERIALS = {"Scales", "Horns", "Eyes", "Membranes"}
FORMATS = {5121: "B", 5123: "H", 5125: "I", 5126: "f"}
WIDTHS = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read_glb(path):
    data = path.read_bytes()
    require(len(data) >= 28, "GLB is truncated")
    require(struct.unpack_from("<4sII", data) == (b"glTF", 2, len(data)), "Invalid GLB header")
    json_length, json_type = struct.unpack_from("<I4s", data, 12)
    require(json_type == b"JSON" and json_length % 4 == 0, "Missing aligned JSON chunk")
    document = json.loads(data[20:20 + json_length])
    bin_start = 20 + json_length
    bin_length, bin_type = struct.unpack_from("<I4s", data, bin_start)
    require(bin_type == b"BIN\x00" and bin_length % 4 == 0, "Missing aligned binary chunk")
    require(bin_start + 8 + bin_length == len(data), "GLB chunk sizes disagree with header")
    declared_size = document["buffers"][0]["byteLength"]
    require(bin_length - 3 <= declared_size <= bin_length, "Invalid buffer length")
    return document, data[bin_start + 8:bin_start + 8 + declared_size]


def read_accessor(document, buffer, index):
    accessor = document["accessors"][index]
    view = document["bufferViews"][accessor["bufferView"]]
    width = WIDTHS[accessor["type"]]
    element_format = "<" + FORMATS[accessor["componentType"]] * width
    element_size = struct.calcsize(element_format)
    view_start = view.get("byteOffset", 0)
    start = accessor.get("byteOffset", 0)
    stride = view.get("byteStride", element_size)
    count = accessor["count"]
    require(view["buffer"] == 0 and view_start + view["byteLength"] <= len(buffer), "View overrun")
    require(count > 0 and stride >= element_size, "Invalid accessor count or stride")
    require(start + (count - 1) * stride + element_size <= view["byteLength"], "Accessor overrun")
    values = [struct.unpack_from(element_format, buffer, view_start + start + i * stride) for i in range(count)]
    require(all(math.isfinite(value) for row in values for value in row), "Non-finite accessor")
    for bound, extremum in (("min", min), ("max", max)):
        if bound in accessor:
            require(all(math.isclose(a, extremum(items), abs_tol=2e-6) for a, items in
                        zip(accessor[bound], zip(*values), strict=True)), f"Invalid {bound} bound")
    return values


def near(a, b, tolerance=1e-5):
    return all(abs(x - y) < tolerance for x, y in zip(a, b, strict=True))


def verify(path):
    document, buffer = read_glb(path)
    require(document["asset"]["version"] == "2.0", "Expected glTF 2.0")
    accessors = [read_accessor(document, buffer, index) for index in range(len(document["accessors"]))]
    require({item["name"] for item in document["materials"]} == MATERIALS, "Wrong PBR material slots")
    require(all("pbrMetallicRoughness" in item for item in document["materials"]), "Missing PBR parameters")

    nodes = document["nodes"]
    (skin,) = document["skins"]
    joints = skin["joints"]
    root = skin["skeleton"]
    require(joints[0] == root and nodes[root]["name"] == "Root", "Missing skin skeleton root")
    require(len(joints) == len(set(joints)), "Duplicate joints")
    require(near(nodes[root].get("translation", (0, 0, 0)), (0, 0, 0)), "Root must stay at ground origin")
    needed = {"Root", "Body", "NeckBase", "NeckTip", "Head", "Jaw", "TailBase", "TailMid", "TailTip",
              *(f"{name}.{side}" for side in ("L", "R") for name in
                ("Hip", "Knee", "Ankle", "WingShoulder", "WingElbow", "WingWrist", "WingDigit"))}
    joint_names = {nodes[index]["name"] for index in joints}
    require(needed <= joint_names, f"Missing articulation: {sorted(needed - joint_names)}")

    world = {}
    def traverse(index, parent):
        require(index not in world, "Cycle or multiple parents in the joint tree")
        node = nodes[index]
        require(near(node.get("rotation", (0, 0, 0, 1)), (0, 0, 0, 1)), "Unexpected bind rotation")
        require(near(node.get("scale", (1, 1, 1)), (1, 1, 1)), "Unexpected bind scale")
        point = tuple(a + b for a, b in zip(parent, node.get("translation", (0, 0, 0)), strict=True))
        world[index] = point
        for child in node.get("children", []):
            traverse(child, point)

    traverse(root, (0, 0, 0))
    bind_matrices = accessors[skin["inverseBindMatrices"]]
    require(len(bind_matrices) == len(joints), "Inverse bind count does not match joint count")
    for joint, matrix in zip(joints, bind_matrices, strict=True):
        x, y, z = world[joint]
        require(near(matrix, (1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, -x, -y, -z, 1)),
                f"Inverse bind mismatch for {nodes[joint]['name']}")

    scene_roots = document["scenes"][document["scene"]]["nodes"]
    (mesh_node,) = (nodes[index] for index in scene_roots if "mesh" in nodes[index])
    require(root in scene_roots and mesh_node["skin"] == 0, "Scene does not contain the skinned mesh and the rig")
    triangles = 0
    all_positions = []
    influenced = set()
    used_materials = set()
    for primitive in document["meshes"][mesh_node["mesh"]]["primitives"]:
        require(primitive.get("mode", 4) == 4, "Expected triangles")
        attrs = {name: accessors[index] for name, index in primitive["attributes"].items()}
        require({"POSITION", "NORMAL", "JOINTS_0", "WEIGHTS_0", "COLOR_0"} <= attrs.keys(),
                "Missing vertex attributes")
        position = attrs["POSITION"]
        require(all(len(values) == len(position) for values in attrs.values()), "Attribute counts disagree")
        require(all(abs(math.hypot(*normal) - 1) < 1e-4 for normal in attrs["NORMAL"]), "Invalid normal")
        require(all(0 <= channel <= 1 for color in attrs["COLOR_0"] for channel in color),
                "Vertex color exceeds the glTF color range")
        indices = [row[0] for row in accessors[primitive["indices"]]]
        require(len(indices) % 3 == 0 and max(indices) < len(position), "Invalid triangle indices")
        for a, b, c in zip(indices[::3], indices[1::3], indices[2::3], strict=True):
            require(len({a, b, c}) == 3, "Collapsed triangle indices")
            ab = [y - x for x, y in zip(position[a], position[b], strict=True)]
            ac = [y - x for x, y in zip(position[a], position[c], strict=True)]
            area = math.hypot(*(ab[i-2] * ac[i-1] - ab[i-1] * ac[i-2] for i in range(3)))
            require(area > 1e-10, "Zero-area triangle")
        material = document["materials"][primitive["material"]]["name"]
        used_materials.add(material)
        membrane_blends = 0
        for vertex_joints, weights in zip(attrs["JOINTS_0"], attrs["WEIGHTS_0"], strict=True):
            require(all(i < len(joints) for i in vertex_joints), "Joint index exceeds skin table")
            require(min(weights) >= 0 and abs(sum(weights) - 1) < 1e-5, "Invalid normalized weights")
            active = {nodes[joints[i]]["name"] for i, weight in zip(vertex_joints, weights, strict=True)
                      if weight > 1e-5}
            influenced |= active
            if sum(name.startswith("Wing") for name in active) >= 2:
                membrane_blends += 1
        if material == "Membranes":
            require(membrane_blends > 20, "Membranes must actually blend across wing joints")
        all_positions.extend(position)
        triangles += len(indices) // 3
    require(used_materials == MATERIALS, "Unused material slot")
    require(needed - {"Root"} <= influenced, "Articulation is missing geometry weights")
    bounds = [(min(axis), max(axis)) for axis in zip(*all_positions, strict=True)]
    require(-0.05 <= bounds[1][0] <= 0.06 and bounds[1][1] > 2, "Model is not grounded at its feet")
    require(6.6 <= bounds[0][1] - bounds[0][0] <= 8.0, "Unexpected wyvern wingspan")
    require(world[next(i for i in joints if nodes[i]["name"] == "Head")][2] > 1 and bounds[2][0] < -2,
            "Creature must face +Z with a trailing tail")

    animations = document["animations"]
    require(len(animations) == len(CLIPS) and {clip["name"] for clip in animations} == CLIPS,
            "Missing or duplicate animation clips")
    for clip in animations:
        name = clip["name"]
        channels = set()
        moving = 0
        end_times = set()
        for channel in clip["channels"]:
            target = channel["target"]
            target_index, property_name = target["node"], target["path"]
            require(target_index in joints and target_index != root, f"{name} animates the model root")
            require((target_index, property_name) not in channels, f"Duplicate channel in {name}")
            channels.add((target_index, property_name))
            sampler = clip["samplers"][channel["sampler"]]
            time = [row[0] for row in accessors[sampler["input"]]]
            samples = accessors[sampler["output"]]
            require(time[0] == 0 and len(time) >= 3 and all(a < b for a, b in zip(time, time[1:])),
                    f"Invalid keyframe times in {name}")
            require(len(time) == len(samples), f"Sampler counts disagree in {name}")
            end_times.add(time[-1])
            moving += any(not near(value, samples[0]) for value in samples[1:])
            if property_name == "rotation":
                require(all(abs(math.hypot(*value) - 1) < 1e-5 for value in samples), f"Bad quaternion in {name}")
            else:
                require(property_name == "translation" and nodes[target_index]["name"] == "Body",
                        f"Unexpected track in {name}")
                rest = nodes[target_index]["translation"]
                require(all(abs(value[0] - rest[0]) < 1e-6 and abs(value[2] - rest[2]) < 1e-6 for value in samples),
                        f"{name} has horizontal root motion")
            if name in {"IdleGround", "Walk", "Flap", "Glide"}:
                require(near(samples[0], samples[-1]), f"{name} does not close at the loop seam")
        require(len(end_times) == 1 and moving >= 5, f"{name} is empty or channels end at different times")
        print(f"{name:10s} {next(iter(end_times)):4.2f}s, {moving:2d} changing tracks")
    print(f"{path.name}: {len(all_positions):,} vertices, {triangles:,} triangles, "
          f"{len(joints)} joints, {bounds[0][1] - bounds[0][0]:.2f} m wingspan")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", nargs="?", type=Path, default=DEFAULT_PATH)
    args = parser.parse_args()
    try:
        verify(args.path)
    except (OSError, ValueError, KeyError, IndexError, TypeError, struct.error) as error:
        parser.exit(1, f"{args.path}: {error}\n")


if __name__ == "__main__":
    main()
