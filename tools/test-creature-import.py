"""Exercise the actual Windows client loader on a GLB, external-buffer glTF and invalid inputs."""
import argparse
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client",type=Path)
    parser.add_argument("--result",type=Path)
    args=parser.parse_args()
    client=args.client.resolve()
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
    main()
