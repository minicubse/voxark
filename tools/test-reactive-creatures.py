"""Exercise neutral server AI against three actual network peers in a disposable world."""
import argparse
from contextlib import ExitStack
import json
import math
from pathlib import Path
import runpy
import tempfile
import time

net = runpy.run_path(str(Path(__file__).with_name("test-creature-network.py")))
request, wait = net["request"], net["await_check"]
pose, teleport, look = net["POSE"], net["teleport"], net["LOOK"]
ready, visual = net["ready"], net["visible_instance"]


def run(args):
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    with tempfile.TemporaryDirectory(dir=args.scratch) as tmp, ExitStack() as stack:
        work = Path(tmp)

        def start(argv, name):
            proc = net["Process"](argv, work / f"{name}.log")
            stack.callback(proc.abort)
            return proc

        config = work / "server.json"
        config.write_text(json.dumps(dict(world=str(work / "forest.pvx"), bind="127.0.0.1:0",
                                          seed=net["SEED"], preset="forest-modern", tick_hz=60, creatures=1)))
        server = start([args.server, "--config", config, "--creature-behavior", "reactive"], "server")
        info = server.event("started")
        assert info["creature_behavior"] == "reactive" and info["tick_hz"] == 60
        first = request({"server": server})["server"]
        assert first["decision"] == ["Rest", None]
        home = first["creatures"][0]["position"]
        cid = first["creatures"][0]["id"]
        manifest = "creatures/wyvern_art.json"

        def join(name, exe):
            options = ["--asset-root", args.assets.resolve(), "--creature-manifest", manifest] if exe == args.bevy else []
            proc = start([exe, "--identity", work / f"{name}.identity", "--test-input",
                          "--connect", info["address"], "--resolution", "960x540",
                          "--capture", work / f"{name}-pending.png", *options], name)
            pid, _ = pose(proc, flying=False)
            proc.send("fly")
            pose(proc)
            return proc, pid

        a, aid = join("bevy-a", args.bevy)
        away = (24.5, 49., 19.5)
        teleport(server, a, aid, away)
        macro, mid = join("macro", args.macro)
        teleport(server, macro, mid, away)
        b, bid = join("bevy-b", args.bevy)
        observer = (home[0]-4., home[1]+9., home[2]+13.)
        teleport(server, b, bid, away)
        clients = {"bevy-a": a, "bevy-b": b}
        peers = {"server": server, **clients}
        history = {key: {} for key in peers}
        events = []
        visuals = {key: {} for key in clients}

        def sample():
            frames = request(peers)
            for name, frame in frames.items():
                if name in clients:
                    net["unique"](frame)
                    if ready(frame):
                        v = visual(frame)
                        visuals[name][v["sample_tick"]] = v
                if net["authoritative"](frame, name):
                    history[name][frame["tick"]] = frame
            return frames

        def stage(name, target=None, clip=None):
            def matches(frames):
                s = frames["server"]
                return (s["decision"] == [name, target] and all(ready(frames[k]) for k in clients)
                        and (not clip or all(visual(frames[k])["clip"] == clip for k in clients)))
            frames = wait(name, sample, matches, timeout=22)
            s = frames["server"]
            events.append(dict(tick=s["tick"], decision=s["decision"], state=s["creatures"][0]))
            print(name, s["tick"], s["creatures"][0]["position"], flush=True)
            return s["creatures"][0]

        for proc in clients.values():
            proc.send("creature-trace on")
        stage("Rest")
        # B watches from outside the acquisition sphere. A enters it in flight mode
        # so a precise authoritative position can be held without camera movement.
        teleport(server, b, bid, observer)
        near = (home[0]+8., home[1]+1., home[2])
        teleport(server, a, aid, near)
        for proc, feet in ((a, near), (b, observer)):
            look(proc, feet, (home[0], home[1]+1.4, home[2]))
        stage("Observe", aid)
        stage("Approach", aid, "Walk")
        until = time.monotonic()+1.
        while time.monotonic()<until:
            sample()
            time.sleep(.015)
        stopped = stage("Stop", aid, "IdleGround")
        assert abs(math.dist(near[::2], stopped["position"][::2])-3.5) < .03
        assert math.dist(stopped["position"], home)>3
        for name, feet in (("bevy-a", near), ("bevy-b", observer)):
            point = stopped["position"]
            look(clients[name], feet, (point[0], point[1]+1.4, point[2]))
            wait("visible art rig", sample, lambda f: ready(f[name]) and visual(f[name])["visible"])
            net["CAPTURE"](clients[name], work / f"{name}-pending.png", output / f"{name}.png")
        teleport(server, a, aid, away)
        stage("Return", clip="Walk")
        back = stage("Rest", clip="IdleGround")
        assert math.dist(back["position"], home)<1e-6
        teleport(server, b, bid, net["FAR"])
        out = wait("interest exit", sample, lambda f: ready(f["bevy-a"]) and
                   not f["bevy-b"]["authoritative"] and not f["bevy-b"]["visuals"])
        teleport(server, b, bid, observer)
        inside = wait("interest return", sample, lambda f: ready(f["bevy-b"]))
        old = visual(inside["bevy-b"])["entity"]
        b.send("reconnect")
        gone = wait("disconnect", sample, lambda f: f["bevy-b"]["player_id"] is None and not f["bevy-b"]["visuals"])
        again = wait("same identity, single root", sample, lambda f: ready(f["bevy-b"]) and f["bevy-b"]["player_id"] == bid)
        assert visual(again["bevy-b"])["entity"] != old and visual(again["bevy-b"])["id"] == cid
        edit = net["edit_with_legacy"](server, a, macro, aid, mid, already_flying=True)
        for name, proc in clients.items():
            proc.send("creature-trace off")
            for _, line in list(proc.lines):
                if line.startswith("CREATURES "):
                    frame = json.loads(line[10:])
                    if ready(frame):
                        history[name][frame["tick"]] = frame
                        v = visual(frame)
                        visuals[name][v["sample_tick"]] = v
        rows = list(net["merge_authority"](history))
        assert {r["server"]["locomotion"] for r in rows} == {"Walk", "IdleGround"}
        for r in rows:
            r["decision"] = history["server"][r["tick"]]["decision"]
        rendered = net["compare_render"](visuals, history)
        assert {"Walk", "IdleGround"} <= {r["clip"] for r in rendered}
        maxima = {k:max(r["difference"][k] for r in rendered) for k in rendered[0]["difference"]}
        assert maxima["position_m"] < .02 and maxima["yaw_rad"] < .02 and maxima["phase_s"] < .01, maxima
        net["dump_rows"](output/"same-tick.jsonl", rows)
        summary = dict(seed=net["SEED"], manifest=manifest, server_hz=info["tick_hz"], home=home,
                       player_ids=[aid,bid,mid], approach_player_feet=near, observer_feet=observer,
                       stages=events, wire_ticks=len(rows), aligned_render_samples=len(rendered),
                       max_render_difference=maxima, interest_ticks=[out["server"]["tick"],inside["server"]["tick"]],
                       reconnect_ticks=[gone["server"]["tick"],again["server"]["tick"]], macroquad_edit=edit,
                       captures=[net["file_metadata"](output/f"{name}.png") for name in clients])
        (output/"acceptance.json").write_text(json.dumps(summary, indent=2)+"\n")
        for proc in (b,a,macro):
            proc.close("quit")
        server.close("stop")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("server", "bevy", "macro", "output"):
        parser.add_argument("--"+name, type=Path, required=True)
    parser.add_argument("--assets", default=Path("assets"), type=Path)
    parser.add_argument("--scratch", type=Path)
    run(parser.parse_args())
