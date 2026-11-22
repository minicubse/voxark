"""Export the authored Wyvern scene using Blender 4.5 LTS in background mode."""

import argparse
import json
from pathlib import Path
import struct
import sys
import tempfile

import bpy


ASSET_DIR = Path(__file__).resolve().parents[3] / "assets/creatures/wyvern_art"
CLIPS = {"IdleGround", "Walk", "Takeoff", "Flap", "Glide", "Land"}
MATERIALS = {"Scales", "Horns", "Eyes", "Membranes"}


def arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=ASSET_DIR / "wyvern.blend")
    parser.add_argument("--output", type=Path, default=ASSET_DIR / "wyvern.glb")
    args = parser.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else [])
    source = args.source.resolve()
    output = args.output.resolve()
    if source.suffix.lower() != ".blend" or not source.is_file():
        parser.error(f"source Blender introuvable : {source}")
    if output.suffix.lower() != ".glb" or output == (ASSET_DIR.parent / "wyvern.glb").resolve():
        parser.error(f"destination GLB artistique requise : {output}")
    return source, output


def check_export(path):
    with path.open("rb") as stream:
        magic, version, _, length, chunk = struct.unpack("<4sIII4s", stream.read(20))
        if (magic, version, chunk) != (b"glTF", 2, b"JSON"):
            raise ValueError("GLB 2.0 attendu")
        document = json.loads(stream.read(length))
    if document["scenes"][document["scene"]].get("name") != "Wyvern":
        raise ValueError("la scène par défaut du GLB doit être Wyvern")
    clips = [clip.get("name") for clip in document.get("animations", [])]
    if len(clips) != len(CLIPS) or set(clips) != CLIPS:
        raise ValueError(f"six animations nommées attendues, animations exportées : {clips}")
    materials = {material.get("name"): material for material in document.get("materials", [])}
    if materials.keys() != MATERIALS or not materials["Membranes"].get("doubleSided"):
        raise ValueError("quatre matériaux nommés attendus, avec Membranes visible des deux côtés")
    for image in document.get("images", []):
        if image.get("mimeType") != "image/png" or "bufferView" not in image:
            raise ValueError("les textures exportées doivent être incorporées au GLB en PNG")


def main():
    source, output = arguments()
    if bpy.app.version[:2] != (4, 5):
        raise RuntimeError(f"Blender 4.5 LTS attendu, version chargée : {bpy.app.version_string}")
    bpy.ops.wm.open_mainfile(filepath=str(source), load_ui=False)
    scene = bpy.data.scenes.get("Wyvern")
    if scene is None or scene.unit_settings.system != "METRIC" or scene.unit_settings.scale_length != 1.0:
        raise ValueError("créer la scène Wyvern avec les unités métriques et une échelle de 1")
    bpy.context.window.scene = scene

    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="wyvern-export-", dir=output.parent) as directory:
        staged = Path(directory) / output.name
        result = bpy.ops.export_scene.gltf(
            filepath=str(staged),
            export_format="GLB",
            use_active_scene=True,
            export_yup=True,
            export_cameras=False,
            export_lights=False,
            export_image_format="AUTO",
            export_tangents=True,
            export_skins=True,
            export_influence_nb=4,
            export_all_influences=False,
            export_animations=True,
            export_animation_mode="ACTIONS",
            export_merge_animation="ACTION",
            export_anim_single_armature=True,
            export_force_sampling=True,
            export_frame_step=1,
            export_anim_slide_to_zero=True,
            export_try_sparse_sk=False,
        )
        if result != {"FINISHED"}:
            raise RuntimeError(f"export GLB interrompu : {result}")
        check_export(staged)
        staged.replace(output)
    print(f"Blender {bpy.app.version_string} : {source} → {output}")


if __name__ == "__main__":
    main()
