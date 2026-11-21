"""Windows mixed-client acceptance through the same crosshair edit handlers as mouse clicks."""
import argparse
import json
import os
from pathlib import Path
import struct
import subprocess
import threading
import time
import zlib


class Process:
    def __init__(self, args, logfile):
        self.log = logfile.open("w", encoding="utf-8")
        self.p = subprocess.Popen(list(map(str,args)),stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,
                                  text=True, encoding="utf-8",errors="replace",env=dict(os.environ,WGPU_BACKEND="vulkan"))
        self.lines = []
        self.cv = threading.Condition()
        def read():
            for line in self.p.stdout:
                with self.cv:
                    self.lines.append((time.perf_counter(),line.strip()))
                    self.log.write(line)
                    self.log.flush()
                    self.cv.notify_all()
        self.reader = threading.Thread(target=read,daemon=True)
        self.reader.start()

    def send(self, text):
        self.p.stdin.write(text+"\n")
        self.p.stdin.flush()

    def observe(self, value, since=0, peers=1):
        deadline=time.perf_counter()+30
        with self.cv:
            while time.perf_counter()<deadline and self.p.poll() is None:
                for stamp,line in reversed(self.lines):
                    if stamp < since: break
                    parts=line.split()
                    if parts[:1]==["OBSERVED"]:
                        actor,others,revision,block,x,y,z=map(int,parts[1:])
                        if others>=peers and block==value and (x,y,z)==(-1,11,4):
                            return stamp,actor,revision
                self.cv.wait(.1)
        raise AssertionError(f"Cell {value} invisible, pid={self.p.pid}, tail={self.lines[-3:]}")

    def close(self, command):
        if self.p.poll() is None:
            self.send(command)
            try:self.p.wait(10)
            except subprocess.TimeoutExpired:
                self.p.kill();self.p.wait()
                raise
        self.reader.join(2)
        self.log.close()
        assert self.p.returncode==0, self.p.returncode


def run(binaries,d):
    d.mkdir(exist_ok=False)
    world=d/"history.pvx"
    cells=[(-2,11,4,3),(-1,11,4,3)]
    data=struct.pack("<4sHHQI",b"PVXD",1,1,20261120,len(cells))+b"".join(struct.pack("<bbbB",*p) for p in cells)
    world.write_bytes(data+struct.pack("<I",zlib.crc32(data)))
    port="127.0.0.1:48307"
    log=[]
    def server(run):
        return Process([binaries/"primal-server.exe","--bind",port,"--world",world],d/f"server-{run}.log")
    def client(kind,run):
        exe=binaries/("primal-client.exe" if kind=="macro" else "primal-client-bevy.exe")
        if not exe.is_file():exe=binaries/("PrimalVoxel.exe" if kind=="macro" else "PrimalVoxel-Bevy.exe")
        p=Process([exe,"--connect",port,
                   "--resolution","800x450","--identity",d/f"{kind}.identity","--test-input","--capture",d/f"{kind}-{run}.png"],d/f"{kind}-{run}.log")
        p.send("watch -1 11 4")
        return p
    def observed(p,value):
        t=time.perf_counter();p.send("watch -1 11 4");return p.observe(value,t)
    def edit(a,b,block,who,prev_rev):
        t=time.perf_counter();a.send(f"click {block}")
        t1,_,ra=a.observe(block,t)
        t2,_,rb=b.observe(block,t)
        assert ra==rb and ra>prev_rev
        log.append(dict(action="edit",client=who,value=block,revision=ra,local_mesh_ms=(t1-t)*1000,remote_mesh_ms=(t2-t)*1000))
        return ra
    s=a=b=None
    try:
        s=server(1);time.sleep(.6);a=client("macro",1);b=client("bevy",1)
        # One participant may have captured the initial cell before the other joined.
        time.sleep(3)
        _,ia,rev=observed(a,3);_,ib,rb=observed(b,3);assert rev==rb and ia!=ib
        a.send("look -0.35 -0.12");b.send("look -0.35 -0.12");time.sleep(.3)
        for author,other,v,name in [(a,b,0,"Macroquad"),(a,b,4,"Macroquad"),(b,a,0,"Bevy"),(b,a,7,"Bevy")]:
            rev=edit(author,other,v,name,rev)
        b.close("quit");b=client("bevy",2);_,again,rb=b.observe(7);assert again==ib and rb==rev
        log.append(dict(action="bevy_reconnect",id=ib,value=7,revision=rb))
        a.close("quit");a=client("macro",2);_,again,ra=a.observe(7);assert again==ia and ra==rev
        log.append(dict(action="macroquad_reconnect",id=ia,value=7,revision=ra))
        a.close("quit");a=None;b.close("quit");b=None;s.close("stop");s=None
        s=server(2);time.sleep(.6);b=client("bevy",3);a=client("macro",3)
        time.sleep(3)
        for p,expected in ((a,ia),(b,ib)):
            _,pid,revision=observed(p,7);assert pid==expected and revision==rev
        log.append(dict(action="server_restart",actors=[ia,ib],value=7,revision=rev))
        # Move the real actors into each other's field of view after the persistence checks.
        for p in (a,b):p.send("fly")
        time.sleep(.3)
        s.send(f"teleport {ia} 0.5 38 0.5");s.send(f"teleport {ib} 0.5 38 5.5")
        time.sleep(1)
        a.send("look 0 -0.16");b.send("look 3.14159 -0.16");time.sleep(.4)
        a.send("photo");b.send("photo")
        from PIL import Image
        for kind in ("macro","bevy"):
            path=d/f"{kind}-3.png"
            deadline=time.perf_counter()+8
            while True:
                try:
                    with Image.open(path) as image:
                        image.verify()
                    break
                except (OSError,SyntaxError):
                    if time.perf_counter()>=deadline:raise
                    time.sleep(.05)
            with Image.open(path) as im:
                w,h=im.size;area=im.convert("RGB").crop((w//2-20,h//2-35,w//2+20,h//2+35))
                pixels=sum(1 for rgb in (area.get_flattened_data() if hasattr(area,"get_flattened_data") else area.getdata()) if max(abs(j-k) for j,k in zip(rgb,(201,135,64)))<5)
                assert pixels>250,(kind,pixels)
                log.append(dict(action="peer_avatar_in_frame",client=kind,verified_pixels=pixels))
        (d/"acceptance.json").write_text(json.dumps(log,indent=2),encoding="utf-8")
        print(json.dumps(log))
    finally:
        for p,cmd in ((a,"quit"),(b,"quit"),(s,"stop")):
            if p and not p.log.closed:p.close(cmd)


if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binaries",type=Path)
    parser.add_argument("result",type=Path,help="New directory for the disposable world and logs")
    a=parser.parse_args();run(a.binaries.resolve(),a.result.resolve())
