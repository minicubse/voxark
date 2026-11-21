"""Capture the actual Bevy client and benchmark sequentially on Windows. Requires psutil."""
import argparse
import datetime
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import tempfile
import time
import psutil

VIEWS = {"A":"0.5,27.8,0.5,-1.66,0.08", "B":"-28.5,33,-12.5,-1.2,0.02",
         "C":"27,46,3,-1.93,-0.22"}
BARE = ["--lighting","off","--shadows","0","--ao","off","--fog","off","--density","0"]
PALETTE = BARE
RAW = [*BARE,"--palette","off","--variation","off"]
PROFILES = {"historical":[], "raw": RAW, "cull-off":[*RAW,"--culling","off"],
            "palette": PALETTE, "vegetation":[*PALETTE,"--density","0.58"],
            "light":["--ao","off","--shadows","0"], "shadows":["--ao","off","--shadows","1"],
            "two":["--ao","off","--shadows","2"], "ao":["--shadows","0","--ao","on"],
            "low":[], "agx":["--tone","agx"]}
def idle(executables):
    forbidden = {"cargo.exe","rustc.exe","blender.exe","primal-client.exe","primal-server.exe",
                 "primal-client-bevy.exe","primalvoxel-bevy.exe","primalvoxel.exe",*executables}
    active = [p.info["name"] for p in psutil.process_iter(["name"])
              if (p.info["name"] or "").lower() in forbidden]
    if active:
        raise RuntimeError(f"Concurrent graphics or build workload: {active}")

def trial(a, case, stem, *, scene="A", screenshot=False, trace=False, traverse=False):
    historical = case=="historical"
    exe = a.baseline if historical else a.client
    idle({exe.name.lower()})
    frames = 60 if screenshot or trace else (6000 if traverse else a.frames)
    route = "drift" if historical else ("forest-traverse" if traverse else ("fixed" if screenshot or trace else "forest-drift"))
    env = dict(os.environ,WGPU_BACKEND="vulkan",RUST_LOG="error,bevy_render::renderer=info")
    if trace:
        env["RUST_LOG"] += ",wgpu_core::command::render=trace,wgpu_core::device::queue=trace"
    with tempfile.TemporaryDirectory() as tmp:
        report, log = Path(tmp)/"report.json", Path(tmp)/"client.log"
        cmd = [str(exe),"--route",route,"--frames",str(frames),"--resolution","1280x720",
               "--render-radius","4","--report",str(report)]
        if not historical:
            cmd += ["--preset","forest-modern","--view",VIEWS[scene],*PROFILES[case]]
        if screenshot:
            cmd += ["--capture",str(a.output/(stem+".png"))]
        samples=[]
        start=time.perf_counter()
        with log.open("wb") as sink, subprocess.Popen(cmd,stdout=sink,stderr=subprocess.STDOUT,env=env) as child:
            proc=psutil.Process(child.pid)
            while child.poll() is None:
                try:
                    mem=proc.memory_info()
                    samples.append([round(time.perf_counter()-start,3),mem.rss,mem.peak_wset])
                except psutil.NoSuchProcess:
                    break
                if time.perf_counter()-start>180:
                    child.kill()
                    raise TimeoutError(stem)
                time.sleep(.12)
        lines=log.read_text(encoding="utf-8",errors="replace").splitlines()
        if child.returncode or any(" ERROR " in line or "panicked at" in line for line in lines):
            raise RuntimeError(f"{stem}: "+str([x[:500] for x in lines if "ERROR" in x or "panicked" in x]+[s[:160] for s in lines[-3:]]))
        data=json.loads(report.read_text())
        if data["device_type"]!="IntegratedGpu" or "Radeon" not in data["adapter"] or data["backend"]!="Vulkan" or (data["empty_frames"] and not traverse):
            raise RuntimeError(f"Invalid hardware or empty frames for {stem}")
        if trace:
            # A single primary view with one clustering populate pass per frame, also with shadows.
            boundaries=[i for i,s in enumerate(lines) if "set_pipeline RenderPipeline with 'clustering populate pipeline'" in s]
            if len(boundaries)<30:
                raise RuntimeError("Missing GPU frame boundaries")
            groups=[lines[i:j] for i,j in zip(boundaries[-22:-2],boundaries[-21:-1])]
            audit=[]
            for group in groups:
                stamp=None
                submit_us=[]
                for line in group:
                    if "Queue::submit" in line:
                        t=datetime.datetime.fromisoformat(line.split()[0].replace("Z","+00:00"))
                        if line.endswith("Queue::submit"):
                            stamp=t
                        elif stamp and "returned submit index" in line:
                            submit_us.append((t-stamp).total_seconds()*1e6)
                            stamp=None
                audit.append(dict(draw_commands=sum(bool(re.search(r"RenderPass::(?:multi_)?draw",l)) for l in group),
                                  submits=len(submit_us),submit_cpu_us=sum(submit_us),
                                  buffer_writes=sum("Queue::write_buffer" in l for l in group)))
            (a.output/(stem+".log")).write_text("\n".join(groups[-1])+"\n",encoding="utf8")
            data["wgpu_frames"]=audit
        data.update(case=case,scene=scene,argv=cmd[1:cmd.index("--report")]+cmd[cmd.index("--report")+2:],
                    rss_sample_s_bytes_peak=samples,peak_rss=max(x[2] for x in samples))
        if not screenshot:
            (a.output/(stem+".json")).write_text(json.dumps(data,separators=(",",":")),encoding="utf8")
        print(stem,round(data["fps_avg"]),round(data["p99_ms"],2),flush=True)

if __name__=="__main__":
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("mode",choices=["capture","preview","measure","trace","traverse"])
    p.add_argument("--client",required=True,type=Path)
    p.add_argument("--baseline",type=Path)
    p.add_argument("--output",required=True,type=Path)
    p.add_argument("--scenes",nargs="+",choices=list(VIEWS),help="Capture selected reference views")
    p.add_argument("--frames",type=int,default=1100)
    p.add_argument("--cases",nargs="+",choices=list(PROFILES),help="Optional subset, in forward order")
    p.add_argument("--repeats",type=int,default=3)
    a=p.parse_args()
    a.output=a.output.resolve()
    a.output.mkdir(parents=True,exist_ok=True)
    if a.mode in ("preview","capture"):
        for scene in a.scenes or VIEWS:
            for case in (["low"] if a.mode=="preview" else ["raw","palette","shadows","ao","low"]):
                trial(a,case,f"{scene}-{case}",scene=scene,screenshot=True)
        if a.mode=="capture" and (not a.scenes or "A" in a.scenes):
            trial(a,"agx","A-agx",screenshot=True)
    elif a.mode=="measure":
        for run in range(a.repeats):
            cases=a.cases or list(PROFILES)
            for case in cases if run%2==0 else reversed(cases):
                trial(a,case,f"run{run+1}-{case}")
    elif a.mode=="trace":
        for case in a.cases or PROFILES:
            trial(a,case,f"gpu-{case}",trace=True)
    else:
        trial(a,"low","traversal",traverse=True)
