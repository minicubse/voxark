"""Exercise neutral server AI against three actual network peers in a disposable world."""
import argparse
from contextlib import ExitStack
import json
import math
from pathlib import Path
import runpy
import subprocess
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

        world = work / "forest.pvx"
        behavior = "reactive"
        if args.navigation_wall:
            subprocess.run([args.navigation_wall.resolve(), work / "wall"], check=True)
            world = work / "wall" / "forest.pvx"
            behavior = "navigation"
        config = work / "server.json"
        config.write_text(json.dumps(dict(world=str(world), bind="127.0.0.1:0",
                                          seed=net["SEED"], preset="forest-modern", tick_hz=60, creatures=1)))
        server = start([args.server, "--config", config, "--creature-behavior", behavior], "server")
        info = server.event("started")
        assert info["creature_behavior"] == behavior and info["tick_hz"] == 60
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
        if args.navigation_wall:
            observer = (home[0]+9., home[1]+12., home[2]-8.)
        teleport(server, b, bid, away)
        clients = {"bevy-a": a, "bevy-b": b}
        peers = {"server": server, **clients}
        history = {key: {} for key in peers}
        events = []
        visuals = {key: {} for key in clients}

        film = []
        filming = False

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
            tick = frames["server"]["tick"]
            if filming and (not film or tick >= film[-1][0]+24):
                from PIL import Image
                frame_path = work / "frame.png"
                net["CAPTURE"](b, work / "bevy-b-pending.png", frame_path)
                with Image.open(frame_path) as image:
                    film.append((tick, image.convert("RGB").resize((640, 360), Image.Resampling.LANCZOS)))
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
        if args.navigation_wall:
            look(b, observer, (-.5, home[1]+.8, 7.))
            filming = True
        stage("Observe", aid)
        stage("Approach", aid, "Walk")
        until = time.monotonic()+1.
        while time.monotonic()<until:
            sample()
            time.sleep(.015)
        if args.navigation_wall:
            # Capture B beside the wall while the authoritative centre is in its lateral passage.
            def crossing(frames):
                x, _, z = frames["server"]["creatures"][0]["position"]
                return -1.4 <= x <= .4 and (z < 5.66 or z > 10.34) and ready(frames["bevy-b"])
            wait("reaching the lateral passage", sample, crossing)

            net["CAPTURE"](b, work / "bevy-b-pending.png", output / "passage.png")
        stopped = stage("Stop", aid, "IdleGround")
        gap = math.dist(near[::2], stopped["position"][::2])
        if args.navigation_wall:
            assert stopped["position"][0] >= .35 and 3.43 <= gap <= 3.51, stopped
        else:
            assert abs(gap-3.5) < .03
        assert math.dist(stopped["position"], home)>3
        for name, feet in (("bevy-a", near), ("bevy-b", observer)):
            point = stopped["position"]
            if name == "bevy-a" or not args.navigation_wall:
                look(clients[name], feet, (point[0], point[1]+1.4, point[2]))
            wait("visible art rig", sample, lambda f: ready(f[name]) and visual(f[name])["visible"])
            net["CAPTURE"](clients[name], work / f"{name}-pending.png", output / f"{name}.png")
        teleport(server, a, aid, away)
        stage("Return", clip="Walk")
        back = stage("Rest", clip="IdleGround")
        assert math.dist(back["position"], home)<1e-6
        filming = False
        if film:
            durations = [round((right[0]-left[0])*1000/60) for left, right in zip(film, film[1:])] + [500]
            film[0][1].save(output / "detour.webp", save_all=True, append_images=[im for _, im in film[1:]],
                            duration=durations, loop=0, quality=82)
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
        if args.navigation_wall:
            for decision in ("Approach", "Return"):
                path = [r["server"]["position"] for r in rows if r["decision"][0] == decision]
                assert any(-1.35 < x < .35 and (z <= 5.65 or z >= 10.35) for x, _, z in path), decision
                assert all(not (-1.35 < x < .35 and 5.65 < z < 10.35) for x, _, z in path)
        net["dump_rows"](output/"same-tick.jsonl", rows)
        summary = dict(behavior=behavior, seed=net["SEED"], manifest=manifest, server_hz=info["tick_hz"], home=home,
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
    parser.add_argument("--navigation-wall", type=Path, help="Release navigation_wall example executable")
    parser.add_argument("--scratch", type=Path)
    run(parser.parse_args())
