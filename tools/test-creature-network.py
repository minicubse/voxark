"""Exercise one server wyvern with two live Bevy windows and a legacy Macroquad client."""
import argparse
from bisect import bisect_left
from collections import Counter
from contextlib import ExitStack
import hashlib
import json
import math
from pathlib import Path
import runpy
import tempfile
import time


REPO = Path(__file__).resolve().parents[1]
MIXED = runpy.run_path(str(Path(__file__).with_name("test-mixed-renderers.py")))
Process = MIXED["Process"]
CAPTURE = MIXED["capture"]
LOOK = MIXED["look"]
POSE = MIXED["observe_pose"]
FOREST_SITES = MIXED["forest_sites"]
ORDER = ["IdleGround", "Walk", "Takeoff", "Flap", "Glide", "Land"]
SEED = 20261120
TICK_HZ = 60
RESOLUTION = "960x540"
NEAR = {"bevy-a": (10.5, 33.0, 17.5), "bevy-b": (-9.5, 36.0, 17.5)}
FAR = (160.5, 52.0, 0.5)


def executable(directory, *names):
    for name in names:
        path = directory / name
        if path.is_file():
            return path
    raise FileNotFoundError(f"Expected one of {names} in {directory}")


def await_check(description, sample, accept, timeout=45):
    deadline = time.perf_counter() + timeout
    while time.perf_counter() < deadline:
        result = sample()
        if accept(result):
            return result
        time.sleep(0.025)
    raise AssertionError(f"{description}; last sample: {result}")


def request(procs, command="creatures"):
    """Send to all processes before waiting so arrival order cannot skew the sampling."""
    times = {}
    for name, process in procs.items():
        times[name] = time.perf_counter()
        process.send(command)

    def match(line):
        if line.startswith("CREATURES "):
            return json.loads(line.removeprefix("CREATURES "))
        if line.startswith("{"):
            value = json.loads(line)
            return value if value.get("event") == "creatures" else None
        return None

    return {
        name: proc.wait_for(match, times[name], timeout=12, what="creature telemetry")[1]
        for name, proc in procs.items()
    }


def angle_diff(a, b):
    return math.remainder(a - b, 2 * math.pi)


def authoritative(snapshot, key):
    return snapshot["creatures" if key == "server" else "authoritative"]


def motion(wyvern):
    return {key: wyvern[key] for key in (
        "id", "species", "palette", "position", "yaw", "locomotion", "state_start_tick",
    )}


def file_metadata(path):
    with path.open("rb") as stream:
        checksum = hashlib.file_digest(stream, "sha256").hexdigest()
    return {"filename": path.name, "bytes": path.stat().st_size, "sha256": checksum}


def visible_instance(frame):
    visual, = frame["visuals"]
    return visual


def unique(frame):
    assert len(frame["authoritative"]) <= 1, frame
    assert len(frame["visuals"]) <= 1, frame
    if frame["authoritative"] and frame["visuals"]:
        assert frame["authoritative"][0]["id"] == frame["visuals"][0]["id"], frame
        visible_instance(frame)


def ready(frame):
    unique(frame)
    return (
        frame["tick"] is not None and len(frame["authoritative"]) == len(frame["visuals"]) == 1
        and visible_instance(frame)["ready"]
    )


def running_clip(frame, after=0):
    return (
        ready(frame) and frame["tick"] > after
        and visible_instance(frame)["clip"] != ORDER[0]
        and visible_instance(frame)["phase_seconds"] > 0.2
        and visible_instance(frame)["sample_tick"] > visible_instance(frame)["state_start_tick"]
    )


def merge_authority(samples):
    common = set.intersection(*(set(stream) for stream in samples.values()))
    for tick in sorted(common):
        row = {"tick": tick}
        for name, stream in samples.items():
            wyvern, = authoritative(stream[tick], name)
            row[name] = motion(wyvern)
        assert row["server"] == row["bevy-a"] == row["bevy-b"], row
        yield row


def lerp(a, b, factor):
    return a + (b - a) * factor


def sample_render_at(trace, times, tick):
    index = bisect_left(times, tick)
    if index < len(times) and times[index] == tick:
        return trace[tick], [tick, tick]
    if not 0 < index < len(times):
        return None
    begin, end = times[index - 1:index + 1]
    left, right = trace[begin], trace[end]
    # Rendering interpolates linearly between successive three-tick snapshots.
    # Interpolating the telemetry across a snapshot corner would cut the curve a second time.
    if math.floor(begin / 3) != math.ceil(end / 3) - 1:
        return None
    if end - begin > 4.5 or any(left[key] != right[key] for key in ("clip", "state_start_tick")):
        return None
    if right["phase_seconds"] < left["phase_seconds"]:
        return None
    mix = (tick - begin) / (end - begin)
    yaw = left["yaw"] + angle_diff(right["yaw"], left["yaw"]) * mix
    return {
        "position": [lerp(a, b, mix) for a, b in zip(left["position"], right["position"])],
        "yaw": yaw, "clip": left["clip"], "state_start_tick": left["state_start_tick"],
        "phase_seconds": lerp(left["phase_seconds"], right["phase_seconds"], mix),
    }, [begin, end]


def compare_render(traces, authority):
    """Comparison on integer render ticks with a nearby bracket at each observer."""
    indexes = {name: sorted(stream) for name, stream in traces.items()}
    rows = []
    snapshots = authority["bevy-a"].keys() & authority["bevy-b"].keys()
    ticks = sorted(tick + offset for tick in snapshots if tick + 3 in snapshots for offset in (1, 2))
    for tick in ticks:
        samples = {name: sample_render_at(stream, indexes[name], tick) for name, stream in traces.items()}
        if not all(samples.values()):
            continue
        a, a_ticks = samples["bevy-a"]
        b, b_ticks = samples["bevy-b"]
        if a["clip"] != b["clip"] or a["state_start_tick"] != b["state_start_tick"]:
            continue
        error = {
            "position_m": math.dist(a["position"], b["position"]),
            "yaw_rad": abs(angle_diff(a["yaw"], b["yaw"])),
            "phase_s": abs(a["phase_seconds"] - b["phase_seconds"]),
        }
        rows.append({
            "tick": tick, "clip": a["clip"], "state_start_tick": a["state_start_tick"],
            "bevy-a": a, "bevy-b": b, "brackets": {"bevy-a": a_ticks, "bevy-b": b_ticks}, "difference": error,
        })
    return rows


def dump_rows(path, rows):
    with path.open("w", encoding="utf-8", newline="\n") as stream:
        for row in rows:
            stream.write(json.dumps(row, separators=(",", ":")) + "\n")


def state_changes(rows):
    changes = []
    for row in rows:
        data = row["server"]
        entry = {"clip": data["locomotion"], "state_start_tick": data["state_start_tick"],
                 "first_matched_tick": row["tick"]}
        if not changes or any(changes[-1][key] != entry[key] for key in ("clip", "state_start_tick")):
            changes.append(entry)
    return changes


def first_cycle(changes):
    for start in range(len(changes) - len(ORDER) + 1):
        sequence = changes[start:start + len(ORDER)]
        if [step["clip"] for step in sequence] == ORDER:
            return sequence
    return None


def teleport(server, proc, player, feet):
    since = time.perf_counter()
    server.send(f"teleport {player} {' '.join(map(str, feet))}")
    assert server.event("teleport", since)["ok"], (player, feet)
    actor, actual = POSE(proc, feet)
    assert actor == player, (actor, player)
    return actual


def photo(name, clients, feet, scratch, result):
    process = clients[name]
    pending = scratch / f"{name}-pending.png"
    target = result / f"{name}.png"
    airborne = {"Flap", "Glide"}
    for _ in range(4):
        frame = await_check(
            "airborne wyvern ready for photography",
            lambda: request({name: process})[name],
            lambda snap: ready(snap) and visible_instance(snap)["clip"] in airborne,
        )
        visual = visible_instance(frame)
        x, y, z = visual["position"]
        LOOK(process, feet, (x, y + 1.2, z))
        aligned = request({name: process})[name]
        if not ready(aligned):
            continue
        visual = visible_instance(aligned)
        if visual["visible"] and visual["clip"] in airborne:
            CAPTURE(process, pending, target)
            from PIL import Image
            with Image.open(target) as image:
                assert image.size == tuple(map(int, RESOLUTION.split("x"))), image.size
            return {"capture": file_metadata(target), "camera_feet": feet, "seen": aligned}
    raise AssertionError(f"{name}: wyvern did not enter main-camera visibility")


def run(server_bin, bevy_bin, legacy_bin, asset_root, result, scratch):
    result.mkdir(parents=True, exist_ok=False)
    scratch.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="creature-network-", dir=scratch))
    print(f"Transient playtest data: {work}", flush=True)
    config = work / "server.json"
    config.write_text(json.dumps({
        "world": str(work / "forest.pvx"), "bind": "127.0.0.1:0", "seed": SEED,
        "preset": "forest-modern", "tick_hz": TICK_HZ, "creatures": 1,
    }), encoding="utf-8")

    with ExitStack() as stack:
        def start(args, name):
            proc = Process(args, work / f"{name}.log")
            stack.callback(proc.abort)
            return proc

        server = start([server_bin, "--config", config], "server")
        started = server.event("started")
        assert started["tick_hz"] == TICK_HZ and started["preset"] == "forest-modern", started

        def client(exe, name):
            options = ["--asset-root", asset_root] if exe == bevy_bin else []
            return start([
                exe, "--connect", started["address"], "--resolution", RESOLUTION,
                "--identity", work / f"{name}.identity", "--test-input",
                "--capture", work / f"{name}-pending.png", *options,
            ], name)

        a = client(bevy_bin, "bevy-a")
        macro = client(legacy_bin, "macro")
        player_a, _ = POSE(a, flying=False)
        a.send("fly")
        POSE(a)
        feet_a = teleport(server, a, player_a, NEAR["bevy-a"])
        first = await_check("first observer already sees a running clip",
                            lambda: request({"bevy-a": a})["bevy-a"], running_clip)
        b = client(bevy_bin, "bevy-b")
        player_b, _ = POSE(b, flying=False)
        b.send("fly")
        POSE(b)
        feet_b = teleport(server, b, player_b, NEAR["bevy-b"])
        macro_id, _ = POSE(macro, flying=False)
        assert len({player_a, player_b, macro_id}) == 3
        clients = {"bevy-a": a, "bevy-b": b}
        processes = {"server": server, **clients}

        def now_game():
            frames = request(clients)
            for frame in frames.values():
                unique(frame)
            return frames

        joined = await_check(
            "late observer joins the existing timeline",
            now_game,
            lambda frames: ready(frames["bevy-a"]) and running_clip(frames["bevy-b"], first["tick"]),
        )
        entity_a = visible_instance(joined["bevy-a"])["entity"]
        entity_b = visible_instance(joined["bevy-b"])["entity"]
        history = {name: {} for name in processes}
        render = {name: {} for name in clients}
        max_origin_error = 0
        last_ticks = {name: -1 for name in processes}
        checked_frames = Counter()

        def sample_cycle():
            nonlocal max_origin_error
            frames = request(processes)
            for name, frame in frames.items():
                tick = frame["tick"]
                if tick is None:
                    continue
                assert tick >= last_ticks[name], (name, last_ticks, frame)
                last_ticks[name] = tick
                if name != "server":
                    unique(frame)
                    checked_frames[name] += 1
                    if ready(frame):
                        visual = visible_instance(frame)
                        origin = frame["origin_blocks"]
                        reconstructed = [local + base for local, base in zip(visual["local_position"], origin)]
                        max_origin_error = max(max_origin_error, math.dist(reconstructed, visual["position"]))
                        assert visual["entity"] == (entity_a if name == "bevy-a" else entity_b), frame
                        render[name][visual["sample_tick"]] = visual
                if authoritative(frame, name):
                    history[name][tick] = frame
            return first_cycle(state_changes(list(merge_authority(history))))

        for client_proc in clients.values():
            client_proc.send("creature-trace on")
        cycle = await_check("same-tick samples across all six states", sample_cycle, bool, timeout=100)
        # Include several landing samples after the first sample that completes the sequence.
        for _ in range(30):
            sample_cycle()
            time.sleep(0.025)
        for name, proc in clients.items():
            proc.send("creature-trace off")
            for _, line in list(proc.lines):
                if line.startswith("CREATURES "):
                    frame = json.loads(line[len("CREATURES "):])
                    if ready(frame):
                        visual = visible_instance(frame)
                        render[name][visual["sample_tick"]] = visual
                        history[name][frame["tick"]] = frame
        synced = list(merge_authority(history))
        rendered = compare_render(render, history)
        wire_counts = Counter(row["server"]["locomotion"] for row in synced)
        render_counts = Counter(row["clip"] for row in rendered)
        assert all(wire_counts[state] >= 2 and render_counts[state] >= 1 for state in ORDER), (
            wire_counts, render_counts,
        )
        maxima = {key: max(row["difference"][key] for row in rendered) for key in rendered[0]["difference"]}
        assert maxima["position_m"] <= 0.02 and maxima["yaw_rad"] <= 0.02 and maxima["phase_s"] <= 0.01, maxima
        assert max_origin_error <= 0.0001, max_origin_error
        dump_rows(result / "same-tick.jsonl", synced)
        dump_rows(result / "rendered.jsonl", rendered)
        print(f"Compared {len(synced)} wire ticks and {len(rendered)} aligned render samples", flush=True)

        photography = {name: photo(name, clients, feet, work, result)
                       for name, feet in (("bevy-a", feet_a), ("bevy-b", feet_b))}

        teleport(server, b, player_b, FAR)
        out = await_check(
            "Bevy B interest exit removes authority and visual together",
            now_game,
            lambda frames: ready(frames["bevy-a"]) and frames["bevy-b"]["tick"] is not None
            and not frames["bevy-b"]["authoritative"] and not frames["bevy-b"]["visuals"],
        )
        teleport(server, b, player_b, NEAR["bevy-b"])
        entered = await_check(
            "Bevy B entered interest and resumed the active phase",
            now_game,
            lambda frames: ready(frames["bevy-a"]) and running_clip(frames["bevy-b"], out["bevy-b"]["tick"]),
        )
        new_entity = visible_instance(entered["bevy-b"])["entity"]
        assert new_entity != entity_b, entered

        b.send("reconnect")
        disconnected = await_check(
            "Bevy B clears visuals while disconnected",
            now_game,
            lambda frames: ready(frames["bevy-a"]) and frames["bevy-b"]["player_id"] is None
            and not frames["bevy-b"]["authoritative"] and not frames["bevy-b"]["visuals"],
        )
        rejoined = await_check(
            "Bevy B reconnects with the same identity and running timeline",
            now_game,
            lambda frames: ready(frames["bevy-a"]) and frames["bevy-b"]["player_id"] == player_b
            and running_clip(frames["bevy-b"], entered["bevy-b"]["tick"]),
        )
        assert visible_instance(rejoined["bevy-b"])["entity"] not in {entity_b, new_entity}, rejoined
        for frames in (out, entered, disconnected, rejoined):
            assert visible_instance(frames["bevy-a"])["entity"] == entity_a, frames
            for name, frame in frames.items():
                if ready(frame):
                    assert visible_instance(frame)["id"] == visible_instance(joined[name])["id"], frames

        # Exercise the legacy renderer's real crosshair, server edit, and both meshers.
        macro.send("fly")
        POSE(macro)
        (trunk, view, aim), *_ = FOREST_SITES()
        teleport(server, macro, macro_id, view)
        watch = "watch " + " ".join(map(str, trunk))
        since = time.perf_counter()
        for proc in (a, macro):
            proc.send(watch)
        _, _, before_a = a.observe(5, since, peers=2, pos=trunk)
        _, _, before_macro = macro.observe(5, since, peers=2, pos=trunk)
        assert before_a == before_macro
        LOOK(macro, view, aim)
        since = time.perf_counter()
        macro.send("click 0")
        seen_macro, actor_macro, after_macro = macro.observe(0, since, peers=2, pos=trunk)
        seen_a, actor_a, after_a = a.observe(0, since, peers=2, pos=trunk)
        assert actor_macro == macro_id and actor_a == player_a and after_a == after_macro > before_macro
        legacy_edit = {
            "position": trunk, "value_before": 5, "value_after": 0, "revision_before": before_macro,
            "revision_after": after_macro, "macroquad_meshed_ms": (seen_macro - since) * 1000,
            "bevy_meshed_ms": (seen_a - since) * 1000,
        }

        for proc in (b, a, macro):
            proc.close("quit")
        server.close("stop")
        summary = {
            "scenario": {"seed": SEED, "preset": started["preset"], "server_tick_hz": TICK_HZ,
                         "resolution": RESOLUTION, "creatures": 1, "backend": "Vulkan"},
            "executables": {name: file_metadata(path) for name, path in (
                ("server", server_bin), ("bevy", bevy_bin), ("legacy_macroquad", legacy_bin),
            )},
            "player_ids": {"bevy-a": player_a, "bevy-b": player_b, "macroquad": macro_id},
            "authority": {"positions_yaw_and_states_exact": True, "same_tick_count": len(synced),
                          "samples_by_state": wire_counts, "cycle": cycle},
            "render": {"samples_by_state": render_counts, "max_difference": maxima,
                       "max_origin_reconstruction_error_m": max_origin_error,
                       "sample_counts": checked_frames, "duplicate_roots": 0},
            "late_join": {"before_bevy_b_started": first, "with_bevy_b": joined},
            "interest": {"outside_feet": FAR, "outside": out, "return_feet": feet_b, "returned": entered},
            "reconnect": {"disconnected": disconnected, "rejoined": rejoined},
            "macroquad_edit": legacy_edit, "photography": photography,
        }
        (result / "acceptance.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
        print(f"Acceptance evidence: {result}", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binaries", type=Path, help="Directory with the new release server and Bevy client")
    parser.add_argument("result", type=Path, help="New directory for acceptance evidence")
    parser.add_argument("--legacy-macroquad", type=Path, required=True,
                        help="Path to a prior released Macroquad executable (v1 protocol)")
    parser.add_argument("--asset-root", type=Path, default=REPO / "assets")
    parser.add_argument("--scratch", type=Path, default=REPO / "benchmarks/tmp",
                        help="Parent for disposable worlds, captures in progress and process logs")
    args = parser.parse_args()
    run(executable(args.binaries, "primal-server.exe"),
        executable(args.binaries, "primal-client-bevy.exe", "PrimalVoxel-Bevy.exe"),
        args.legacy_macroquad.resolve(), args.asset_root.resolve(), args.result.resolve(), args.scratch.resolve())
