"""Exercise the actual Windows client loader on a GLB, external-buffer glTF and invalid inputs."""
import argparse
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import sys


def probe(client, manifest, asset_root):
    root = asset_root.resolve(strict=True)
    manifest = manifest.resolve(strict=True)
    relative = manifest.relative_to(root).as_posix()
    definition = json.loads(manifest.read_text(encoding="utf8"))
    with tempfile.TemporaryDirectory() as folder:
        report = Path(folder) / "client.json"
        run = subprocess.run(
            [client.resolve(strict=True), "--asset-root", root, "--creature-manifest", relative,
             "--creatures", "1", "--frames", "15", "--view", "3,28,10,-2.53,-0.13", "--report", report],
            capture_output=True, timeout=90, encoding="utf8", errors="replace",
            env=dict(os.environ, WGPU_BACKEND="vulkan"),
        )
        errors = [line for line in run.stderr.splitlines() if "ERROR" in line or "non charg" in line]
        if run.returncode or errors or not report.exists():
            raise ValueError(f"Bevy (code {run.returncode}) : " + ("\n".join(errors) or run.stderr)[-1600:])
        native = json.loads(report.read_text(encoding="utf8"))
    counts = native["creatures"]
    info = native["creature_import"]
    if not (info["model"] == definition["model"] and info["clips"] == definition["clips"]
            and (definition.get("scene") is None or info["scene"] == definition["scene"])
            and all(info["clip_durations_seconds"][name] > 0 for name in definition["clips"])
            and counts["clips"] == len(definition["clips"]) and counts["slots"] == len(definition["slots"])
            and counts["players"] == counts["instances"] == 1 and counts["skins"] >= 1
            and counts["max_joints"] > 0 and counts["skinned_meshes"] > 0):
        raise ValueError(f"Import ou instanciation incomplet : {counts}, {info}")
    return {"valid": True, "manifest": relative, "adapter": native["adapter"],
            "backend": native["backend"], "counts": counts, "import": info}

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("legacy_client", nargs="?", type=Path, help="Original eight-case regression suite")
    parser.add_argument("--client", type=Path, help="Precompiled Bevy validation client")
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--asset-root", type=Path, default=Path("assets"))
    parser.add_argument("--result",type=Path)
    args=parser.parse_args()
    if args.manifest or args.client:
        if not args.manifest or not args.client or args.legacy_client:
            parser.error("Use --client EXE --manifest FILE [--asset-root DIR], or the legacy positional EXE")
        try:
            result = probe(args.client, args.manifest, args.asset_root)
        except (OSError, ValueError, KeyError, TypeError, subprocess.TimeoutExpired) as exc:
            result = {"valid": False, "error": str(exc), "manifest": str(args.manifest)}
        encoded = json.dumps(result, indent=2, ensure_ascii=False)
        if args.result:
            args.result.parent.mkdir(parents=True, exist_ok=True)
            args.result.write_text(encoded + "\n", encoding="utf8")
        print(encoded)
        return 0 if result["valid"] else 1
    if not args.legacy_client:
        parser.error("Specify the validation client")
    client=args.legacy_client.resolve()
    source=Path(__file__).resolve().parents[1]/"assets/creatures"
    template=json.loads((source/"wyvern.json").read_text(encoding="utf8"))
    glb=(source/"wyvern.glb").read_bytes()
    n=struct.unpack_from("<I",glb,12)[0]
    gltf=json.loads(glb[20:20+n])
    evidence=[]
    with tempfile.TemporaryDirectory() as temporary:
        root=Path(temporary)
        assets=root/"creatures"
        assets.mkdir()
        (assets/"wyvern.glb").write_bytes(glb)
        gltf["buffers"][0]["uri"]="skin%20data.bin"
        (assets/"skin data.bin").write_bytes(glb[28+n:])
        (assets/"replacement.gltf").write_text(json.dumps(gltf),encoding="utf8")
        (assets/"broken.glb").write_bytes(b"not a glTF")
        for case,model,valid in [
            ("valid_glb","wyvern.glb",True),("external_gltf","replacement.gltf",True),
            ("missing_manifest","",False),("broken_manifest","",False),
            ("missing_model","absent.glb",False),("invalid_glb","broken.glb",False),
            ("unknown_clip","wyvern.glb",False),("unknown_slot","wyvern.glb",False),
        ]:
            manifest=json.loads(json.dumps(template))
            manifest["model"]="creatures/"+model
            if case=="unknown_clip":
                manifest["clips"]["Walk"]="missing"
            if case=="unknown_slot":
                manifest["slots"]["Scales"]="missing"
            name=assets/(case+".json")
            if case!="missing_manifest":
                name.write_text("{" if case=="broken_manifest" else json.dumps(manifest),encoding="utf8")
            report=root/"report.json"
            report.unlink(missing_ok=True)
            cmd=[client,"--asset-root",root,"--creature-manifest","creatures/"+name.name,"--creatures","1",
                 "--frames","15","--view","3,28,10,-2.53,-0.13","--report",report]
            run=subprocess.run(cmd,capture_output=True,timeout=75,env=dict(os.environ,WGPU_BACKEND="vulkan"),
                               encoding="utf8",errors="replace")
            if valid:
                if run.returncode or not report.is_file():
                    raise RuntimeError(f"{case}: {run.stdout[-600:]} {run.stderr[-900:]}")
                counts=json.loads(report.read_text())["creatures"]
                assert counts["players"]==1 and counts["skins"]==1 and counts["clips"]==6 and counts["slots"]==4,counts
                evidence.append(dict(case=case,loaded=True,counts=counts))
            else:
                message=next((line for line in run.stderr.splitlines() if "non charg" in line),"")
                if run.returncode==0 or not message:
                    raise RuntimeError(f"{case}: {run.returncode} {run.stderr[-1200:]}")
                evidence.append(dict(case=case,loaded=False,exit_code=run.returncode,diagnostic=message))
            print(case,"ok",flush=True)
    if args.result:
        args.result.parent.mkdir(parents=True,exist_ok=True)
        args.result.write_text(json.dumps(evidence,ensure_ascii=False,indent=2),encoding="utf8")

if __name__=="__main__":
    sys.exit(main())
