"""Alternate the two Windows clients on one dedicated server using identical camera samples."""
import argparse
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import subprocess
import time


class MemoryCounters(ctypes.Structure):
    _fields_=[("cb",wintypes.DWORD),("PageFaultCount",wintypes.DWORD)]+[(n,ctypes.c_size_t) for n in
              ("PeakWorkingSetSize","WorkingSetSize","QuotaPeakPagedPoolUsage","QuotaPagedPoolUsage",
               "QuotaPeakNonPagedPoolUsage","QuotaNonPagedPoolUsage","PagefileUsage","PeakPagefileUsage")]


def run(bins,d,frames,rounds):
    d.mkdir(exist_ok=False,parents=True)
    rows=[]
    addr="127.0.0.1:48317"
    with (d/"server.log").open("w") as log:
        server=subprocess.Popen([bins/"primal-server.exe","--bind",addr,"--world",d/"benchmark.pvx"],stdin=subprocess.PIPE,
                                text=True,stdout=log,stderr=log)
        try:
            time.sleep(.6)
            for route in ("fixed","drift"):
                for i in range(rounds):
                    pair=[]
                    for client in (("macroquad","bevy") if i%2==0 else ("bevy","macroquad")):
                        tag=f"{route}-{i+1}-{client}"
                        report=d/(tag+".json")
                        exe=bins/("primal-client.exe" if client=="macroquad" else "primal-client-bevy.exe")
                        command=[exe,"--connect",addr,"--identity",d/"same.identity","--compare","--compare-route",route,"--net-frames",str(frames),
                                 "--resolution","1280x720","--render-radius","4","--net-report",report]
                        with (d/(tag+".log")).open("w") as clog:
                            p=subprocess.Popen(command,stdout=clog,stderr=clog,env=dict(os.environ,WGPU_BACKEND="vulkan"))
                            peak=0;deadline=time.monotonic()+100
                            try:
                                while p.poll() is None:
                                    counter=MemoryCounters();counter.cb=ctypes.sizeof(counter)
                                    if ctypes.windll.psapi.GetProcessMemoryInfo(wintypes.HANDLE(p._handle),ctypes.byref(counter),counter.cb):
                                        peak=max(peak,counter.PeakWorkingSetSize)
                                    if time.monotonic()>deadline:raise RuntimeError(f"timeout {tag}")
                                    time.sleep(.04)
                            finally:
                                if p.poll() is None:p.kill();p.wait()
                        if p.returncode or not report.is_file():raise RuntimeError(f"missing report or failed client: {tag}")
                        data=json.loads(report.read_text(encoding="utf-8"));data.update(peak_rss_bytes=peak,repetition=i+1)
                        assert peak>0 and data["players"]==1 and data["empty_frames"]==0
                        rows.append(data);pair.append(data);time.sleep(.35)
                    for field in ("visible_chunks","quads","triangles","resolution","route","geometry_digest"):
                        assert pair[0][field]==pair[1][field],(route,i,field,pair[0][field],pair[1][field])
            (d/"comparison.json").write_text(json.dumps(rows,indent=2),encoding="utf-8")
        finally:
            if server.poll() is None:
                server.stdin.write("stop\n");server.stdin.flush();server.wait(15)


if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binaries",type=Path);parser.add_argument("result",type=Path)
    parser.add_argument("--frames",type=int,default=720);parser.add_argument("--rounds",type=int,default=4)
    a=parser.parse_args();run(a.binaries.resolve(),a.result.resolve(),a.frames,a.rounds)
