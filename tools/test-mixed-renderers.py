"""Windows mixed-client acceptance through the same crosshair edit handlers as mouse clicks."""
import argparse
from contextlib import ExitStack
import json
import math
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

    def wait_for(self, match, since=0, timeout=30, what="matching event"):
        deadline=time.perf_counter()+timeout
        with self.cv:
            while True:
                for stamp,line in reversed(self.lines):
                    if stamp < since: break
                    result=match(line)
                    if result is not None:
                        return stamp,result
                remaining=deadline-time.perf_counter()
                if remaining<=0 or self.p.poll() is not None:
                    break
                self.cv.wait(min(remaining,.1))
        raise AssertionError(f"No {what}, pid={self.p.pid}, tail={self.lines[-6:]}")

    def observe(self, value, since=0, peers=1, pos=(-1,11,4)):
        def match(line):
            if line.startswith("OBSERVED "):
                actor,others,revision,block,x,y,z=map(int,line.split()[1:])
                if others>=peers and block==value and (x,y,z)==pos:
                    return actor,revision
            return None
        stamp,(actor,revision)=self.wait_for(match,since,what=f"meshed cell {pos} = {value}")
        return stamp,actor,revision

    def event(self, event, since=0):
        def match(line):
            if line.startswith("{"):
                value=json.loads(line)
                if value.get("event")==event:
                    return value
            return None
        return self.wait_for(match,since,what=event)[1]

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

    def abort(self):
        if not self.log.closed:
            if self.p.poll() is None:
                self.p.kill()
                self.p.wait()
            self.reader.join(2)
            self.log.close()


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


def angles(feet, point):
    x,y,z=(v-p for v,p in zip(point,(feet[0],feet[1]+1.62,feet[2])))
    return math.atan2(x,z),math.atan2(y,math.hypot(x,z))


def look(client, feet, point):
    yaw,pitch=angles(feet,point)
    client.send(f"look {yaw} {pitch}")


def observe_pose(client, expected=None, flying=True):
    def match(line):
        if not line.startswith("POSE "):
            return None
        _,actor,flight,x,y,z=line.split()
        return int(actor),flight=="true",tuple(map(float,(x,y,z)))

    deadline=time.perf_counter()+30
    while time.perf_counter()<deadline:
        since=time.perf_counter()
        client.send("pose")
        try:
            _,(actor,flight,feet)=client.wait_for(match,since,timeout=.5,what="authoritative player pose")
        except AssertionError:
            if client.p.poll() is not None:
                raise
            continue
        if flight==flying and (expected is None or math.dist(expected,feet)<0.05):
            return actor,feet
        time.sleep(.05)
    raise AssertionError(f"Expected pose {expected} (flying={flying}), last events {client.lines[-3:]}")


def capture(client, pending, destination):
    from PIL import Image

    pending.unlink(missing_ok=True)
    client.send("photo")
    deadline=time.perf_counter()+12
    while True:
        try:
            with Image.open(pending) as image:
                image.verify()
            pending.replace(destination)
            return destination
        except (OSError,SyntaxError):
            if time.perf_counter()>=deadline:
                raise
            time.sleep(.05)


def check_decoration(before, removed, restored):
    from PIL import Image

    crops=[]
    for path in (before,removed,restored):
        with Image.open(path) as image:
            width,height=image.size
            # The camera centers the decoration above its support. Inspect the
            # narrow area around that point, leaving the voxel beneath it out.
            center_x,center_y=width//2,height//2
            region=(center_x-28,center_y-29,center_x+28,center_y+5)
            crop=image.convert("RGB").crop(region)
            pixels=crop.get_flattened_data() if hasattr(crop,"get_flattened_data") else crop.getdata()
            crops.append(list(pixels))

    def changed(a,b):
        return sum(max(abs(x-y) for x,y in zip(p,q))>12 for p,q in zip(a,b))

    lost=changed(crops[0],crops[1])
    recovered=changed(crops[1],crops[2])
    difference=changed(crops[0],crops[2])
    assert lost>=50 and recovered>=50 and difference<=min(lost,recovered)//10, (lost,recovered,difference)
    return dict(removed_pixels=lost,restored_pixels=recovered,difference_after_restore=difference)


def forest_sites():
    trunk = ((-17, 28, 2), (-13.5, 26.58, 2.5), (-16.5, 28.5, 2.5))
    leaf = ((-17, 37, 15), (-13.5, 35.58, 15.5), (-16.5, 37.5, 15.5))
    remote = {
        "macro": ((4101, 64, 5), (4099.5, 64.08, 7.5), (4101.5, 63.5, 5.5), 4),
        "bevy": ((-4091, 55, 5), (-4088.5, 55.08, 7.5), (-4090.5, 54.5, 5.5), 7),
    }
    support = ((2,24,4), (2.5,26.,4.5), (2.5,24.5,4.5), (2.5,23.5,4.5), 1)
    decoration = ((4.5,25.08,6.5),(2.5,25.28,4.5),(8.5,32.,10.5))
    return trunk, leaf, support, decoration, remote


def run_forest(binaries,d):
    d.mkdir(exist_ok=False)
    world=d/"forest.pvx"
    config=d/"server.json"
    config.write_text(json.dumps(dict(world=str(world),bind="127.0.0.1:0",seed=20261120)),encoding="utf-8")
    names={"macro":"Macroquad","bevy":"Bevy"}
    build_exes={"macro":"primal-client.exe","bevy":"primal-client-bevy.exe"}
    package_exes={"macro":"PrimalVoxel.exe","bevy":"PrimalVoxel-Bevy.exe"}
    log=[]

    with ExitStack() as stack:
        def start(args,logfile):
            process=Process(args,logfile)
            stack.callback(process.abort)
            return process

        def server(run,initialize=False):
            options=["--preset","forest-modern"] if initialize else []
            process=start([binaries/"primal-server.exe","--config",config,*options],d/f"server-{run}.log")
            info=process.event("started")
            assert info["preset"]=="forest-modern",info
            return process,info["address"]

        def client(kind,run):
            exe=binaries/build_exes[kind]
            if not exe.is_file():
                exe=binaries/package_exes[kind]
            return start([exe,"--connect",address,"--resolution","800x450","--identity",d/f"{kind}.identity",
                          "--test-input","--capture",d/f"{kind}-capture.png"],d/f"{kind}-{run}.log")

        def watch(kind,pos,value):
            process=clients[kind]
            since=time.perf_counter()
            process.send(f"watch {pos[0]} {pos[1]} {pos[2]}")
            return process.observe(value,since,pos=pos)[2]

        def watch_both(pos,value):
            revisions={watch(kind,pos,value) for kind in clients}
            assert len(revisions)==1,(pos,value,revisions)
            return revisions.pop()

        def move(kind,feet):
            since=time.perf_counter()
            serving.send(f"teleport {actors[kind]} {' '.join(map(str,feet))}")
            info=serving.event("teleport",since)
            assert info["ok"],(kind,feet,info)
            actor,actual=observe_pose(clients[kind],feet)
            assert actor==actors[kind],(kind,actor,actors[kind])
            return actual

        def edit(kind,pos,old,new,feet,point,action):
            revision=watch_both(pos,old)
            look(clients[kind],feet,point)
            since=time.perf_counter()
            clients[kind].send(f"click {new}")
            evidence={}
            for viewer,process in clients.items():
                stamp,actor,new_revision=process.observe(new,since,pos=pos)
                assert actor==actors[viewer] and new_revision>revision,(viewer,actor,new_revision,revision)
                evidence[viewer]=(stamp,new_revision)
            assert len({r for _,r in evidence.values()})==1,evidence
            revision=evidence[kind][1]
            log.append(dict(action=action,client=names[kind],cell=pos,old=old,value=new,revision=revision,
                            meshed_ms={names[viewer]:round((stamp-since)*1000,2) for viewer,(stamp,_) in evidence.items()}))
            return revision

        serving,address=server(1,initialize=True)
        clients={kind:client(kind,1) for kind in names}
        actors={kind:observe_pose(process,flying=False)[0] for kind,process in clients.items()}
        assert len(set(actors.values()))==2,actors
        for kind,process in clients.items():
            process.send("fly")
            observe_pose(process)

        # Fixed coordinates in the 20261120 forest. Feet and aim points come
        # from the actual generator and are checked through the real crosshair.
        trunk,leaf,support,decoration,remote_sites=forest_sites()
        trunk_cell,trunk_feet,trunk_aim=trunk
        leaf_cell,leaf_feet,leaf_aim=leaf
        support_cell,support_feet,support_remove,support_place,support_material=support
        photo_feet,photo_aim,parking=decoration

        move("macro",trunk_feet)
        trunk_rev=edit("macro",trunk_cell,5,0,trunk_feet,trunk_aim,"remove_trunk")
        move("bevy",leaf_feet)
        leaf_rev=edit("bevy",leaf_cell,6,0,leaf_feet,leaf_aim,"remove_leaf")

        images={kind:{} for kind in clients}
        def plant(expected):
            p=clients["bevy"]
            since=time.perf_counter()
            p.send("plant "+" ".join(map(str,support_cell)))
            _,answer=p.wait_for(lambda line:line if line.startswith("PLANT ") else None,since)
            assert answer.split()[-1] == str(expected).lower(),answer
            log.append(dict(action="published_flora_support",cell=support_cell,present=expected))

        def photograph(stage):
            watch_both(support_cell,0 if stage.endswith("removed") else support_material)
            plant(not stage.endswith("removed"))
            for kind,process in clients.items():
                other=next(name for name in clients if name!=kind)
                move(other,parking)
                move(kind,photo_feet)
                look(process,photo_feet,photo_aim)
                time.sleep(.25)
                images[kind][stage]=capture(process,d/f"{kind}-capture.png",d/f"{kind}-{stage}.png")

        watch_both(support_cell,support_material)
        photograph("decoration-before")
        move("macro",support_feet)
        edit("macro",support_cell,support_material,0,support_feet,support_remove,"remove_decoration_support")
        photograph("decoration-removed")
        move("bevy",support_feet)
        edit("bevy",support_cell,0,3,support_feet,support_place,"place_incompatible_stone")
        plant(False)
        edit("bevy",support_cell,3,0,support_feet,support_remove,"remove_incompatible_stone")
        support_rev=edit("bevy",support_cell,0,support_material,support_feet,support_place,"restore_decoration_support")
        photograph("decoration-restored")
        evidence=check_decoration(*(images["bevy"][f"decoration-{stage}"] for stage in ("before","removed","restored")))
        log.append(dict(action="decoration_disappears_and_returns",client="Bevy",**evidence))

        # Both edits are in flight concurrently while the players are in
        # separate saved regions. Swap the regions to read each other's edit.
        releases={}
        for kind,(pos,feet,point,material) in remote_sites.items():
            move(kind,feet)
            old=watch(kind,pos,0)
            look(clients[kind],feet,point)
            releases[kind]=(pos,old,material)
        submissions={}
        for kind,(_,_,material) in releases.items():
            submissions[kind]=time.perf_counter()
            clients[kind].send(f"click {material}")
        committed={}
        observations=[]
        for kind,(pos,old,material) in releases.items():
            stamp,actor,revision=clients[kind].observe(material,submissions[kind],pos=pos)
            assert revision>old and actor==actors[kind],(kind,actor,old,revision)
            committed[kind]=revision
            observations.append(stamp)
        assert max(submissions.values())<min(observations),"Both edits must be sent before either is observed"
        log.append(dict(action="concurrent_cross_region_placements",clients=names,
                        cells={k:v[0] for k,v in releases.items()},revisions=committed))
        for kind in clients:
            other=next(name for name in clients if name!=kind)
            pos,feet,_,material=remote_sites[other]
            move(kind,feet)
            assert watch(kind,pos,material)==committed[other],(kind,pos,material)
            log.append(dict(action="cross_region_read",client=names[kind],cell=pos,value=material,revision=committed[other]))

        def verify_remote(kind):
            other=next(name for name in clients if name!=kind)
            pos,feet,_,material=remote_sites[other]
            actor,actual=observe_pose(clients[kind],feet)
            assert actor==actors[kind],(kind,actor,actors[kind])
            assert watch(kind,pos,material)==committed[other],(kind,pos,material)
            return dict(client=names[kind],actor=actor,feet=actual,cell=pos,value=material,revision=committed[other])

        for kind in clients:
            clients[kind].close("quit")
            clients[kind]=client(kind,2)
            log.append(dict(action="reconnect",**verify_remote(kind)))
        for process in clients.values():
            process.close("quit")
        serving.close("stop")
        regions=sorted(p.name for p in world.with_suffix(".pvx.regions").glob("r.*.pvr"))
        assert len(regions)>=3,regions

        serving,address=server(2)
        clients={kind:client(kind,3) for kind in names}
        for process in clients.values():
            observe_pose(process)
        log.append(dict(action="server_restart",preset="forest-modern",regions=regions,
                        players=[verify_remote(kind) for kind in clients]))
        for kind in clients:
            move(kind,parking)
        # The three edited cells may share a chunk. Its final revision is
        # whichever later edit last touched that chunk.
        forest_edits=((trunk_cell,0,trunk_rev),(leaf_cell,0,leaf_rev),(support_cell,support_material,support_rev))
        for pos,value,_ in forest_edits:
            chunk=tuple(n//16 for n in pos)
            expected=max(r for p,_,r in forest_edits if tuple(n//16 for n in p)==chunk)
            assert watch_both(pos,value)==expected,(pos,value,expected)
        log.append(dict(action="forest_edits_after_restart",cells=[p for p,_,_ in forest_edits]))
        for process in clients.values():
            process.close("quit")
        serving.close("stop")

    (d/"acceptance.json").write_text(json.dumps(log,indent=2),encoding="utf-8")
    print(json.dumps(log))


if __name__=="__main__":
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binaries",type=Path)
    parser.add_argument("result",type=Path,help="New directory for the disposable world and logs")
    parser.add_argument("--forest",action="store_true",help="Exercise a new forest-modern world")
    a=parser.parse_args()
    (run_forest if a.forest else run)(a.binaries.resolve(),a.result.resolve())
