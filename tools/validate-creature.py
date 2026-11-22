"""Validate a Primal Voxel creature manifest and its GLB or glTF assets."""

import argparse
from collections import Counter
import json
import math
from pathlib import Path, PurePosixPath
import struct
from urllib.parse import unquote

from creature_gltf import (
    Gltf, ValidationError, array, index_of, integer, number, object_value, png_dimensions, read_json, require, vector,
)


CLIPS = ("IdleGround", "Walk", "Takeoff", "Flap", "Glide", "Land")
SLOTS = ("Scales", "Horns", "Eyes", "Membranes")
PALETTES = ("Jade", "Braise", "Obsidienne")
MAX_JOINTS = 256  # bevy::pbr::MAX_JOINTS
CONVENTIONS = {
    "source_forward": "+Z",
    "up_axis": "+Y",
    "units": "meters",
    "import_correction": "bevy_gltf_scene_rotation",
    "pivot": "ground",
    "root_motion": "in_place",
}
SUPPORTED_REQUIRED_EXTENSIONS = {
    "KHR_lights_punctual", "KHR_materials_emissive_strength", "KHR_materials_ior",
    "KHR_materials_transmission", "KHR_materials_unlit", "KHR_texture_transform",
}
IDENTITY = (1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1)
MANUAL_CHECKS = (
    "Verify +Z faces forward and +Y faces up on an asymmetric model in the Bevy viewer.",
    "Verify ground pivot and dimensions visually, including held poses and wing clearance.",
    "Inspect bind pose and deformation, silhouette, materials, and both sides of membranes.",
    "Inspect stationary movement, looping seams, and takeoff and landing transitions in the Bevy viewer.",
    "Run the precompiled Bevy import test to verify decoding, instantiation, and animation playback.",
)


def validate_manifest(manifest):
    required = {"schema_version", "model", "scale", "clips", "slots", "palettes", *CONVENTIONS}
    missing = required - manifest.keys()
    unknown = manifest.keys() - required - {"scene"}
    require(not missing, f"Missing manifest fields: {', '.join(sorted(missing))}")
    require(not unknown, f"Unknown manifest fields: {', '.join(sorted(unknown))}")
    require(type(manifest["schema_version"]) is int and manifest["schema_version"] == 1,
            "Unknown manifest schema version; expected 1")
    model = manifest["model"]
    require(isinstance(model, str) and not any(char in model for char in ":#\\\0")
            and all(part not in ("", ".", "..") for part in model.split("/"))
            and PurePosixPath(model).suffix in (".glb", ".gltf"), "Expected a relative GLB/glTF model path")
    require(number(manifest["scale"], "manifest.scale") > 0
            and struct.pack("<f", manifest["scale"]) != b"\0" * 4, "Expected a positive manifest scale")
    for key, value in CONVENTIONS.items():
        require(manifest[key] == value, f"Manifest {key} must be {value!r}")
    scene = manifest.get("scene")
    require(scene is None or isinstance(scene, str) and scene.strip(), "Manifest scene must be a nonempty string")
    for field, keys in (("clips", CLIPS), ("slots", SLOTS)):
        values = object_value(manifest[field], f"manifest.{field}")
        require(values.keys() == set(keys), f"Manifest {field} must have exactly: {', '.join(keys)}")
        require(all(isinstance(value, str) and value.strip() for value in values.values())
                and len(set(values.values())) == len(keys), f"Manifest {field} names must be nonempty and distinct")
    palettes = object_value(manifest["palettes"], "manifest.palettes")
    require(palettes.keys() == set(PALETTES), f"Manifest palettes must have exactly: {', '.join(PALETTES)}")
    for name, palette in palettes.items():
        object_value(palette, f"palette {name}")
        require(palette.keys() == set(SLOTS), f"Palette {name} must define all material slots")
        for slot, rgb in palette.items():
            require(all(0 <= value <= 1 for value in vector(rgb, 3, f"palette {name}/{slot}")),
                    f"Palette {name}/{slot} requires sRGB values between 0 and 1")


def quaternion(values, label):
    require(math.isclose(math.hypot(*values), 1.0, rel_tol=0.001), f"{label} must be a unit quaternion")


def affine(values, label):
    require(all(abs(values[index] - IDENTITY[index]) <= 1e-6 for index in (3, 7, 11, 15)),
            f"{label} must be affine")


def transform(node):
    if "matrix" in node:
        require(not node.keys() & {"translation", "rotation", "scale"}, "Node cannot combine a matrix with TRS")
        matrix = vector(node["matrix"], 16, "node matrix")
        affine(matrix, "node matrix")
        columns = [matrix[index:index + 3] for index in (0, 4, 8)]
        for first, second in ((0, 1), (0, 2), (1, 2)):
            u, v = columns[first], columns[second]
            dot = sum(a * b for a, b in zip(u, v))
            require(abs(dot) <= math.hypot(*u) * math.hypot(*v) * 1e-5, "Node matrix contains shear")
        return matrix
    x, y, z, w = vector(node.get("rotation", [0, 0, 0, 1]), 4, "node rotation")
    quaternion((x, y, z, w), "node rotation")
    sx, sy, sz = vector(node.get("scale", [1, 1, 1]), 3, "node scale")
    translation = vector(node.get("translation", [0, 0, 0]), 3, "node translation")
    return (
        sx * (1 - 2 * (y * y + z * z)), sx * 2 * (x * y + z * w), sx * 2 * (x * z - y * w), 0,
        sy * 2 * (x * y - z * w), sy * (1 - 2 * (x * x + z * z)), sy * 2 * (y * z + x * w), 0,
        sz * 2 * (x * z + y * w), sz * 2 * (y * z - x * w), sz * (1 - 2 * (x * x + y * y)), 0,
        *translation, 1,
    )


def multiply(left, right):
    return tuple(sum(left[row + 4 * inner] * right[inner + 4 * column] for inner in range(4))
                 for column in range(4) for row in range(4))


def validate_extensions(document):
    used = array(document.get("extensionsUsed", []), "extensionsUsed")
    required = array(document.get("extensionsRequired", []), "extensionsRequired")
    require(all(isinstance(name, str) for name in used + required), "Extension names must be strings")
    require(set(required) <= set(used), "Required extensions must also be listed in extensionsUsed")
    unsupported = set(required) - SUPPORTED_REQUIRED_EXTENSIONS
    require(not unsupported, f"Required glTF extensions unsupported by Bevy: {', '.join(sorted(unsupported))}")


class CreatureModel:
    def __init__(self, gltf, manifest, limits):
        self.gltf = gltf
        self.manifest = manifest
        self.limits = limits
        self.tables = gltf.tables
        self.nodes = self.tables["nodes"]
        self.parents = {}
        self.children = []
        self.paths = {}
        self.world = {}
        self.selected = set()
        self.scene_roots = set()
        self.rig_roots = set()
        self.scene_joints = set()
        self.scene_skins = set()

    def validate(self, report):
        metrics = report["metrics"]
        validate_extensions(self.gltf.document)
        metrics["scene"] = self.read_hierarchy()
        metrics["textures"] = self.read_textures()
        material_uvs = self.read_materials()
        skins = self.read_skins()
        meshes = self.read_meshes(material_uvs)
        self.check_geometry(meshes, skins, metrics)
        metrics["clips"] = self.check_animations(meshes)
        self.gltf.validate_remaining_accessors()

    def read_hierarchy(self):
        roots = set(range(len(self.nodes)))
        for index, node in enumerate(self.nodes):
            children = array(node.get("children", []), f"node[{index}].children")
            for child in children:
                child = index_of(self.nodes, child, f"node[{index}] child")
                require(child not in self.parents, f"Node {child} has multiple parents or duplicate links")
                self.parents[child] = index
                roots.discard(child)
            self.children.append(children)
        stack = [(root, (), IDENTITY) for root in roots]
        while stack:
            index, ancestors, parent = stack.pop()
            path = (*ancestors, index)
            self.paths[index] = path
            world = multiply(parent, transform(self.nodes[index]))
            require(all(math.isfinite(value) for value in world), f"Node {index} has a non-finite world transform")
            self.world[index] = world
            stack.extend((child, path, world) for child in self.children[index])
        require(len(self.paths) == len(self.nodes), "Cycle in the node hierarchy")
        scenes = self.tables["scenes"]
        scene_names = Counter(entry.get("name") for entry in scenes)
        name = self.manifest.get("scene")
        selected = self.gltf.document.get("scene")
        if selected is not None:
            index_of(scenes, selected, "default scene")
        if name is not None:
            require(scene_names[name] == 1, f"Expected one scene named {name!r}, found {scene_names[name]}")
            selected = next(index for index, scene in enumerate(scenes) if scene.get("name") == name)
        require(selected is not None, "Manifest omits scene and the glTF has no default scene")
        for index, scene in enumerate(scenes):
            scene_roots = array(scene.get("nodes", []), f"scene[{index}].nodes")
            for root in scene_roots:
                require(index_of(self.nodes, root, f"scene[{index}] root") in roots,
                        f"scene[{index}] references a node that has a parent")
            require(len(scene_roots) == len(set(scene_roots)), f"scene[{index}] has duplicate roots")
            if index == selected:
                self.scene_roots = set(scene_roots)
        self.selected = {index for index, path in self.paths.items() if path[0] in self.scene_roots}
        require(self.selected, "Selected glTF scene has no nodes")
        return scenes[selected].get("name", f"Scene{selected}")

    def read_textures(self):
        images = []
        for index, image in enumerate(self.tables["images"]):
            label = f"image[{index}]"
            require(("uri" in image) != ("bufferView" in image), f"{label} must have one URI or bufferView")
            require(image.get("mimeType") in (None, "image/png"), f"{label}: Bevy requires PNG textures")
            if "uri" in image:
                source = image["uri"]
                data, mime = self.gltf.resource(source)
                require(mime in (None, "image/png"), f"{label}: Bevy requires PNG textures")
                if mime is not None:
                    source = "data:image/png"
                else:
                    require(PurePosixPath(unquote(source)).suffix.lower() == ".png", f"{label} needs a .png file extension")
            else:
                require(image.get("mimeType") == "image/png", f"{label} must declare its PNG MIME type")
                view = self.gltf.ref("bufferViews", image["bufferView"], label)
                data, _, stride = self.gltf.views[view]
                require(stride is None, f"{label} bufferView must not be interleaved")
                source = f"bufferView[{view}]"
            width, height = png_dimensions(data, label)
            limit = self.limits["max_texture_size"]
            require(max(width, height) <= limit, f"Texture budget exceeded for {label}: {width} x {height}, limit {limit} per side")
            images.append({"image": index, "source": source, "width": width, "height": height})
        for index, sampler in enumerate(self.tables["samplers"]):
            for key, allowed in (
                ("magFilter", (9728, 9729)), ("minFilter", (9728, 9729, 9984, 9985, 9986, 9987)),
                ("wrapS", (33071, 33648, 10497)), ("wrapT", (33071, 33648, 10497)),
            ):
                require(key not in sampler or sampler[key] in allowed, f"Invalid texture sampler[{index}].{key}")
        for index, texture in enumerate(self.tables["textures"]):
            self.gltf.ref("images", texture.get("source"), f"texture[{index}].source")
            if "sampler" in texture:
                self.gltf.ref("samplers", texture["sampler"], f"texture[{index}].sampler")
        return images

    def read_materials(self):
        material_uvs = []
        for index, material in enumerate(self.tables["materials"]):
            label = f"material[{index}]"
            pbr = object_value(material.get("pbrMetallicRoughness", {}), f"{label}.pbrMetallicRoughness")
            require(material.get("alphaMode", "OPAQUE") in ("OPAQUE", "MASK", "BLEND"), f"Invalid {label}.alphaMode")
            require(type(material.get("doubleSided", False)) is bool, f"Invalid {label}.doubleSided")
            require(number(material.get("alphaCutoff", 0.5), f"{label}.alphaCutoff") >= 0, f"Invalid {label}.alphaCutoff")
            for field, length in (("baseColorFactor", 4), ("emissiveFactor", 3)):
                values = (pbr if length == 4 else material).get(field, [0] * length)
                require(all(0 <= value <= 1 for value in vector(values, length, f"{label}.{field}")),
                        f"Invalid {label}.{field} range")
            for field in ("metallicFactor", "roughnessFactor"):
                require(0 <= number(pbr.get(field, 1), f"{label}.{field}") <= 1, f"Invalid {label}.{field} range")
            infos = []
            for properties, fields in (
                (material, ("normalTexture", "occlusionTexture", "emissiveTexture")),
                (pbr, ("baseColorTexture", "metallicRoughnessTexture")),
            ):
                infos.extend(object_value(properties[key], f"{label}.{key}") for key in fields if key in properties)
            texcoords = set()
            for info in infos:
                self.gltf.ref("textures", info.get("index"), f"{label} texture")
                coord = integer(info.get("texCoord", 0), f"{label} texture coordinates")
                require(coord <= 1, f"{label} uses UV set {coord}; Bevy supports only TEXCOORD_0 and TEXCOORD_1")
                texcoords.add(coord)
            material_uvs.append(texcoords)
        return material_uvs

    def read_skins(self):
        skins = []
        for index, skin in enumerate(self.tables["skins"]):
            label = f"skin[{index}]"
            joints = [index_of(self.nodes, node, f"{label} joint") for node in array(skin.get("joints"), f"{label}.joints", 1)]
            require(len(joints) <= MAX_JOINTS and len(set(joints)) == len(joints),
                    f"{label} must have 1 to {MAX_JOINTS} distinct joints")
            joint_paths = [self.paths[joint] for joint in joints]
            common_path = []
            for nodes in zip(*joint_paths):
                if len(set(nodes)) != 1:
                    break
                common_path.append(nodes[0])
            require(common_path, f"{label} joints must share a hierarchy")
            skeleton = skin.get("skeleton", common_path[-1])
            require(index_of(self.nodes, skeleton, f"{label} skeleton") in common_path,
                    f"{label} skeleton must be an ancestor of every joint")
            if "inverseBindMatrices" in skin:
                matrices = self.gltf.values(skin["inverseBindMatrices"], f"{label} inverse bind matrices",
                                            ("MAT4",), (5126,), False)
                require(len(matrices) == len(joints), f"{label} inverse bind matrix count does not match joint count")
                for joint, matrix in zip(joints, matrices):
                    name = f"{label} bind matrix for node {joint}"
                    affine(matrix, name)
                    a, b, c, _, d, e, f, _, g, h, i = matrix[:11]
                    determinant = a * (e * i - f * h) - d * (b * i - c * h) + g * (b * f - c * e)
                    require(determinant != 0, f"{name} is singular")
            skins.append((joints, skeleton))
        return skins

    def read_meshes(self, material_uvs):
        meshes = []
        for index, mesh in enumerate(self.tables["meshes"]):
            primitives = []
            morph_count = None
            for primitive in array(mesh.get("primitives"), f"mesh[{index}].primitives", 1):
                label = f"mesh[{index}] primitive[{len(primitives)}]"
                object_value(primitive, label)
                attributes = object_value(primitive.get("attributes"), f"{label}.attributes")
                require(not any(key.startswith(("JOINTS_", "WEIGHTS_")) and key not in ("JOINTS_0", "WEIGHTS_0")
                                for key in attributes), f"{label}: at most four influences per vertex are supported")
                position = self.gltf.values(attributes.get("POSITION"), f"{label}.POSITION", ("VEC3",), (5126,), False)
                bounds = self.tables["accessors"][attributes["POSITION"]]
                require("min" in bounds and "max" in bounds, f"{label}.POSITION is missing bounds")
                for name, attribute in attributes.items():
                    data = self.gltf.values(attribute, f"{label}.{name}")
                    require(len(data) == len(position), f"{label}.{name} vertex count differs from POSITION")
                    if name == "NORMAL":
                        self.gltf.values(attribute, f"{label}.NORMAL", ("VEC3",), (5126,), False)
                        require(all(math.isclose(math.hypot(*normal), 1, abs_tol=0.002) for normal in data),
                                f"{label} has non-unit normals")
                    elif name == "TANGENT":
                        self.gltf.values(attribute, f"{label}.TANGENT", ("VEC4",), (5126,), False)
                        require(all(abs(row[3]) == 1 and math.isclose(math.hypot(*row[:3]), 1, abs_tol=0.002)
                                    for row in data), f"{label} has invalid tangents")
                    elif name.startswith(("TEXCOORD_", "COLOR_")):
                        self.normalized_attribute(attribute, f"{label}.{name}", ("VEC2",) if name.startswith("TEXCOORD_")
                                                  else ("VEC3", "VEC4"))
                joint_index = attributes.get("JOINTS_0")
                weight_index = attributes.get("WEIGHTS_0")
                require((joint_index is None) == (weight_index is None), f"{label} requires both joint indices and weights")
                joints = weights = None
                if joint_index is not None:
                    joints = self.gltf.values(joint_index, f"{label} joint indices", ("VEC4",), (5121, 5123), False)
                    weights = self.normalized_attribute(weight_index, f"{label} weights", ("VEC4",))
                    for vertex_joints, influences in zip(joints, weights):
                        active = [joint for joint, weight in zip(vertex_joints, influences) if weight > 0]
                        require(all(0 <= value <= 1 for value in influences) and abs(sum(influences) - 1) <= 0.015,
                                f"{label} has invalid normalized skin weights")
                        require(len(active) == len(set(active)), f"{label} repeats an active joint index at a vertex")
                material = None
                if "material" in primitive:
                    material = self.gltf.ref("materials", primitive["material"], f"{label} material")
                    require(all(f"TEXCOORD_{coord}" in attributes for coord in material_uvs[material]),
                            f"{label} is missing UV coordinates used by its material")
                targets = array(primitive.get("targets", []), f"{label}.targets")
                require(morph_count in (None, len(targets)), f"{label} has a different number of morph targets")
                morph_count = len(targets)
                for target in targets:
                    object_value(target, f"{label} morph target")
                    require(target and target.keys() <= {"POSITION", "NORMAL", "TANGENT"}, f"Invalid {label} morph target")
                    for name, attribute in target.items():
                        data = self.gltf.values(attribute, f"{label} morph {name}", ("VEC3",), (5126,), False)
                        require(name in attributes and len(data) == len(position), f"{label} morph {name} has incompatible vertices")
                triangles = self.count_triangles(primitive, position, label)
                primitives.append((len(position), triangles, material, joints))
            if "weights" in mesh:
                vector(mesh["weights"], morph_count, f"mesh[{index}].weights")
            meshes.append((primitives, morph_count))
        return meshes

    def normalized_attribute(self, attribute, label, shape):
        accessor = self.tables["accessors"][self.gltf.ref("accessors", attribute, label)]
        return self.gltf.values(attribute, label, shape, (5121, 5123, 5126), accessor.get("componentType") != 5126)

    def count_triangles(self, primitive, position, label):
        mode = integer(primitive.get("mode", 4), f"{label}.mode")
        require(mode in (0, 1, 3, 4, 5), f"{label}: primitive mode {mode} is unsupported by Bevy")
        indices = range(len(position))
        if "indices" in primitive:
            rows = self.gltf.values(primitive["indices"], f"{label} indices", ("SCALAR",), (5121, 5123, 5125), False)
            indices = [index for (index,) in rows]
            require(max(indices) < len(position), f"{label} has a vertex index outside POSITION")
        count = len(indices)
        minimum = {0: 1, 1: 2, 3: 2, 4: 3, 5: 3}[mode]
        require(count >= minimum and (mode not in (1, 4) or count % minimum == 0), f"{label} has an invalid primitive index count")
        if mode == 4:
            # Triangle strips may deliberately contain degenerate connector triangles.
            for a, b, c in zip(indices[::3], indices[1::3], indices[2::3]):
                pa, pb, pc = position[a], position[b], position[c]
                u = [y - x for x, y in zip(pa, pb)]
                v = [y - x for x, y in zip(pa, pc)]
                require(len({a, b, c}) == 3 and any(u[i-2] * v[i-1] != u[i-1] * v[i-2] for i in range(3)),
                        f"{label} has a collapsed triangle")
            return count // 3
        return count - 2 if mode == 5 else 0

    def check_geometry(self, meshes, skins, metrics):
        used_materials = set()
        vertices = triangles = mesh_instances = 0
        for index, node in enumerate(self.nodes):
            label = f"node[{index}]"
            if "camera" in node:
                self.gltf.ref("cameras", node["camera"], f"{label}.camera")
            require("mesh" in node or not node.keys() & {"skin", "weights"}, f"{label} needs a mesh when skinned or morphed")
            if "mesh" not in node:
                continue
            primitives, morphs = meshes[index_of(meshes, node["mesh"], f"{label}.mesh")]
            if "weights" in node:
                vector(node["weights"], morphs, f"{label}.weights")
            joints = None
            if "skin" in node:
                skin = index_of(skins, node["skin"], f"{label}.skin")
                joints, skeleton = skins[skin]
                if index in self.selected:
                    require(set(joints) <= self.selected, f"{label} has joints outside the selected scene")
                    self.scene_skins.add(skin)
                    self.rig_roots.update(self.paths[skeleton])
                    self.scene_joints.update(joints)
            if index in self.selected:
                mesh_instances += 1
            for count, faces, material, vertex_joints in primitives:
                if joints is not None:
                    require(vertex_joints is not None, f"{label} skinned primitive is missing joint indices or weights")
                    require(all(joint < len(joints) for row in vertex_joints for joint in row), f"{label} joint index exceeds skin size")
                if index in self.selected:
                    triangles += faces
                    vertices += count
                    if material is not None:
                        used_materials.add(material)
        metrics.update(triangles=triangles, vertices=vertices, mesh_instances=mesh_instances, skins=len(self.scene_skins),
                       max_joints=max((len(skins[index][0]) for index in self.scene_skins), default=0))
        limit = self.limits["max_triangles"]
        require(0 < triangles <= limit, f"Triangle budget: found {triangles}, expected 1 to {limit}")
        require(self.scene_skins, "No skinned mesh in the selected scene")
        material_names = Counter(self.tables["materials"][index].get("name") for index in used_materials)
        for slot, name in self.manifest["slots"].items():
            require(material_names[name] == 1, f"Expected one used material for slot {slot} ({name!r}), found {material_names[name]}")
        metrics["slots"] = self.manifest["slots"]

    def check_animations(self, meshes):
        table = self.tables["animations"]
        names = Counter(animation.get("name") for animation in table)
        wanted = self.manifest["clips"]
        for clip, name in wanted.items():
            require(names[name] == 1, f"Expected one animation for clip {clip} ({name!r}), found {names[name]}")
        animations = {}
        animated_roots = set()
        animated_nodes = set()
        for index, animation in enumerate(table):
            name = animation.get("name")
            label = f"clip {name!r}" if name is not None else f"animation[{index}]"
            channels = array(animation.get("channels"), f"{label}.channels", 1)
            samplers = array(animation.get("samplers"), f"{label}.samplers", 1)
            for sampler in samplers:
                object_value(sampler, f"{label} sampler")
                self.gltf.ref("accessors", sampler.get("input"), f"{label} sampler input")
                self.gltf.ref("accessors", sampler.get("output"), f"{label} sampler output")
            tracks = set()
            duration = 0
            for channel in channels:
                object_value(channel, f"{label} channel")
                sampler = samplers[index_of(samplers, channel.get("sampler"), f"{label} channel sampler")]
                target = object_value(channel.get("target"), f"{label} channel target")
                node = index_of(self.nodes, target.get("node"), f"{label} target node")
                field = target.get("path")
                require(field in ("translation", "rotation", "scale", "weights"), f"{label} has an invalid target path")
                require((node, field) not in tracks, f"{label} has duplicate {field} tracks on node {node}")
                tracks.add((node, field))
                require(field == "weights" or "matrix" not in self.nodes[node], f"{label} targets TRS on a matrix node")
                end, data, cubic = self.read_track(sampler, node, field, meshes, label)
                duration = max(duration, end)
                if field == "translation" and name in wanted.values() and node in self.rig_roots:
                    self.check_root_motion(node, data, cubic, label)
                if self.paths[node][0] in self.scene_roots:
                    animated_roots.add(self.paths[node][0])
                    animated_nodes.add(node)
            if name in wanted.values():
                targets = {node for node, _ in tracks}
                require(duration > 0 and targets <= self.selected and targets & self.scene_joints,
                        f"{label} needs a positive duration, targets within the selected scene, and a skin joint track")
                animations[name] = {"name": name, "duration_seconds": duration, "tracks": len(tracks)}
        require(len(animated_roots) == 1, f"Bevy requires one animation player in the selected scene, found {len(animated_roots)}")
        animation_root = next(iter(animated_roots))
        require(all(self.paths[joint][0] == animation_root for joint in self.scene_joints),
                "All selected skin joints must belong to the Bevy animation player")
        target_paths = {}
        targets = self.scene_joints | animated_nodes
        for node in self.selected:
            if self.paths[node][0] == animation_root:
                path = tuple(self.nodes[index].get("name", f"GltfNode{index}") for index in self.paths[node])
                if path in target_paths:
                    require(node not in targets and target_paths[path] not in targets,
                            f"Ambiguous Bevy animation node path: {' / '.join(path)}")
                target_paths[path] = node
        return {clip: animations[name] for clip, name in wanted.items()}

    def read_track(self, sampler, node, field, meshes, label):
        times = self.gltf.values(sampler["input"], f"{label} timestamps", ("SCALAR",), (5126,), False, allow_sparse=False)
        time_accessor = self.tables["accessors"][sampler["input"]]
        require("min" in time_accessor and "max" in time_accessor, f"{label} timestamp accessor is missing bounds")
        timestamps = [time for (time,) in times]
        require(timestamps[0] >= 0 and all(left < right for left, right in zip(timestamps, timestamps[1:])),
                f"{label} timestamps must be non-negative and strictly increasing")
        interpolation = sampler.get("interpolation", "LINEAR")
        require(interpolation in ("LINEAR", "STEP", "CUBICSPLINE"), f"{label} has invalid interpolation")
        cubic = interpolation == "CUBICSPLINE"
        require(not cubic or len(times) >= 2, f"{label} cubic spline requires at least two keyframes")
        multiplier = 3 if cubic else 1
        output = sampler["output"]
        if field == "rotation":
            accessor = self.tables["accessors"][output]
            data = self.gltf.values(output, f"{label} rotation", ("VEC4",), (5120, 5121, 5122, 5123, 5126),
                                    accessor.get("componentType") != 5126)
            for rotation in (data[1::3] if cubic else data):
                quaternion(rotation, f"{label} rotation")
        elif field == "weights":
            mesh = self.nodes[node].get("mesh")
            require(mesh is not None and meshes[mesh][1] > 0, f"{label} weight track has no morph targets")
            multiplier *= meshes[mesh][1]
            accessor = self.tables["accessors"][output]
            data = self.gltf.values(output, f"{label} morph weights", ("SCALAR",), (5120, 5121, 5122, 5123, 5126),
                                    accessor.get("componentType") != 5126)
        else:
            data = self.gltf.values(output, f"{label} {field}", ("VEC3",), (5126,), False)
        require(len(data) == len(times) * multiplier, f"{label} {field} output count does not match keyframes")
        return timestamps[-1] if len(times) > 1 else 0, data, cubic

    def check_root_motion(self, node, data, cubic, label):
        parent = self.world[self.parents[node]] if node in self.parents else IDENTITY
        values = data[1::3] if cubic else data

        def distance(delta):
            projected = [sum(parent[axis + 4 * col] * delta[col] for col in range(3)) for axis in (0, 2)]
            return math.hypot(*projected) * self.manifest["scale"]

        def displacement(row):
            return distance([after - before for before, after in zip(values[0], row)])

        require(displacement(values[-1]) <= 0.001,
                f"{label} has horizontal root motion at node {node}")
        if node in self.scene_roots and node not in self.scene_joints:
            tangents = data[2:-1:3] + data[3::3] if cubic else []
            require(all(displacement(row) <= 0.001 for row in values)
                    and all(distance(tangent) <= 0.001 for tangent in tangents),
                    f"{label} has horizontal root motion at scene root node {node}")


def positive_int(value):
    result = int(value)
    if result <= 0:
        raise argparse.ArgumentTypeError("expected a positive integer")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--asset-root", type=Path, help="Bevy asset directory (default: nearest assets directory or repository assets)")
    parser.add_argument("--result", type=Path, help="Also write the JSON report to this file")
    parser.add_argument("--max-triangles", default=18_000, type=positive_int)
    parser.add_argument("--max-texture-size", default=2048, type=positive_int, help="Maximum texture size per side in pixels")
    args = parser.parse_args()
    report = {
        "valid": False,
        "manifest": str(args.manifest),
        "limits": {"max_triangles": args.max_triangles, "max_texture_size": args.max_texture_size},
        "metrics": {},
        "errors": [],
        "manual_checks": list(MANUAL_CHECKS),
    }
    try:
        manifest_path = args.manifest.resolve()
        default_root = Path(__file__).resolve().parents[1] / "assets"
        root = (args.asset_root or next((parent for parent in manifest_path.parents if parent.name == "assets"), default_root)).resolve()
        manifest = read_json(manifest_path.read_bytes(), "Creature manifest")
        validate_manifest(manifest)
        report["model"] = manifest["model"]
        model = (root / manifest["model"]).resolve()
        require(model.is_relative_to(root), "Model path escapes the asset root")
        CreatureModel(Gltf(model, root), manifest, report["limits"]).validate(report)
        report["valid"] = True
    except (ValueError, OSError, RecursionError) as error:
        report["errors"].append(str(error))
    if args.result:
        try:
            args.result.parent.mkdir(parents=True, exist_ok=True)
            args.result.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
        except OSError as error:
            report["valid"] = False
            report["errors"].append(f"Could not write report: {error}")
    print(json.dumps(report, indent=2))
    return int(not report["valid"])


if __name__ == "__main__":
    raise SystemExit(main())
