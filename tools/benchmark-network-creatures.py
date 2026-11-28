"""Measure a single networked Bevy observer of an authoritative forest on Windows (psutil)."""
import argparse
from contextlib import ExitStack
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
from runpy import run_path
import statistics
import subprocess
import tempfile
import time

import psutil

TOOLS = Path(__file__).resolve().parent
mixed = run_path(str(TOOLS / "test-mixed-renderers.py"))
Process = mixed["Process"]
helpers = run_path(str(TOOLS / "benchmark-creatures.py"))
SEED = 20_261_120
VIEW = "19,43,25,-2.52,-0.40"


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def hardware():
    script = """
    [pscustomobject]@{
      cpu=(Get-CimInstance Win32_Processor).Name
      memory_bytes=(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory
      os=(Get-CimInstance Win32_OperatingSystem).Caption
      adapters=@(Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion)
      power_plan=(powercfg /getactivescheme | Out-String).Trim()
    } | ConvertTo-Json -Depth 4 -Compress
    """
    return json.loads(subprocess.check_output(["powershell.exe", "-NoProfile", "-Command", script],
                                              text=True, errors="replace"))


def trial(args, number, count, control, host, behavior=None):
    server = args.old_server if control else args.server
    if behavior == "reactive" and args.reactive_server:
        server = args.reactive_server
    client = args.old_client if control else args.client
    label = f"r{number}-" + ("3d-online-0" if control else f"4a-online-{count}")
    if behavior:
        label += "-" + behavior
    if args.mixed_macroquad:
        label += "-mixed"
    output = args.output / (label + ".json")
    if output.exists():
        raise FileExistsError(f"Existing measurement: {output}")
    with tempfile.TemporaryDirectory() as tmp, ExitStack() as peers:
        directory = Path(tmp)
        config = directory / "config.json"
        config.write_text(json.dumps(dict(bind="127.0.0.1:0", seed=SEED, tick_hz=60,
                                          preset="forest-modern", world=str(directory / "world.pvx"))))
        argv = [server, "--config", config]
        if not control:
            argv.extend(["--creatures", count])
        if behavior:
            argv.extend(["--creature-behavior", behavior])
        serving = Process(argv, directory / "server.log")
        try:
            info = serving.event("started")
            assert info["preset"] == "forest-modern", info
            if behavior:
                assert info["creature_behavior"] == behavior
            report = directory / "client.json"
            cmd = [client, "--connect", info["address"], "--net-frames", str(args.frames), "--net-report", report,
                   "--identity", directory / "player.identity", "--asset-root", args.assets,
                   "--preset", "forest-modern", "--route", "fixed", "--view", VIEW,
                   "--resolution", "1280x720", "--render-radius", "4"]
            if args.creature_manifest and not control:
                cmd.extend(["--creature-manifest", args.creature_manifest])
            companions = []
            if args.mixed_macroquad:
                # Both stay connected and render throughout the primary client's measurement.
                for name, exe in (("observer", client), ("legacy", args.mixed_macroquad)):
                    options = ["--asset-root", args.assets, "--render-radius", "4",
                               "--creature-manifest", args.creature_manifest] if name == "observer" else []
                    peer = Process([exe, "--connect", info["address"], "--identity", directory / (name+".identity"),
                                    "--test-input", "--resolution", "1280x720", *options], directory / (name+".log"))
                    peers.callback(peer.abort)
                    mixed["observe_pose"](peer, flying=False)
                    companions.append(peer)
            expected_players = 1 + len(companions)
            exitcode, samples = helpers["sample_process"](list(map(str, cmd)), directory / "client.log",
                                                         dict(os.environ, WGPU_BACKEND="vulkan"), 160)
            end = time.perf_counter()
            if exitcode or not report.exists():
                raise RuntimeError((directory / "client.log").read_text(errors="replace")[-2200:])
            native = json.loads(report.read_text())
            assert native["players"] == expected_players and native["empty_frames"] == 0
            assert native["backend"] == "Vulkan" and native["device_type"] != "Cpu"
            assert native["resolution"] == [1280, 720] and native["render_radius"] == 4
            assert native["frames"] == args.frames and native["route"] == "fixed"
            if count:
                assert native["creatures"]["instances"] == count, native.get("creatures")
                assert native["creature_inventory"]["joints_all_instances"] == count * 23
            measured_s = args.frames / native["fps_avg"]
            # Discard windows crossing warmup or final disconnect. End time includes
            # a small amount of client shutdown; the full-period selection avoids partial rates.
            windows = []
            previous_sent = 0
            for stamp, line in list(serving.lines):
                if not line.startswith("{"):
                    continue
                event = json.loads(line)
                if event.get("event") != "metrics":
                    continue
                event["all_period_sent_bytes"] = event["sent_bytes"] - previous_sent
                previous_sent = event["sent_bytes"]
                if (event["players"] == expected_players and event["samples"] == 120
                        and end - measured_s <= stamp - event["period_s"] and stamp <= end):
                    windows.append(event)
            if not windows:
                raise RuntimeError("No full 120-tick server period during measurement; increase --frames")
            result = dict(repetition=number, mode="3d-online" if control else "4a-online", count=count,
                          measured_seconds=measured_s, seed=SEED, view=VIEW, profile="Low", host=host,
                          client_sha256=sha256(client), server_sha256=sha256(server),
                          client_cpu_cores=native["process_cpu_ms"] / (1000 * measured_s),
                          client_peak_rss_bytes=max(max(row[1],row[2] or 0) for row in samples),
                          server_rss_end_bytes=psutil.Process(serving.p.pid).memory_info().rss,
                          server_windows=windows, client=native)
            if behavior:
                result["behavior"] = behavior
                result["manifest"] = args.creature_manifest
            if companions:
                result["mixed_peers"] = True
                result["macroquad_sha256"] = sha256(args.mixed_macroquad)
                for peer in companions:
                    peer.close("quit")
            serving.close("stop")
        finally:
            serving.abort()
    helpers["write_json"](output, result)
    print(f"{label}: {native['fps_avg']:.1f} FPS, 1% low {native['fps_1low']:.1f}, "
          f"P99 {native['p99_ms']:.2f} ms", flush=True)
    return result


def summarize(rows):
    groups = {}
    for row in rows:
        name = f"{row['mode']}-{row['count']}"
        if "behavior" in row:
            name += "-" + row["behavior"]
        if row.get("mixed_peers"):
            name += "-mixed"
        native = row["client"]
        windows = row["server_windows"]
        seconds = sum(w["period_s"] for w in windows)
        fields = {k:native[k] for k in ("fps_avg", "fps_1low", "p99_ms")}
        fields.update(client_cpu_cores=row["client_cpu_cores"], client_peak_mib=row["client_peak_rss_bytes"]/2**20,
                      server_resident_mib=row["server_rss_end_bytes"]/2**20,
                      server_tick_p50_ms=statistics.median(w["tick_p50_ms"] for w in windows),
                      server_worst_window_p99_ms=max(w["tick_p99_ms"] for w in windows),
                      server_actual_hz=sum(w["samples"] for w in windows)/seconds,
                      server_cpu_cores=sum(w["cpu_ms"] for w in windows)/(seconds*1000),
                      server_sent_bytes_s=sum(w["all_period_sent_bytes"] for w in windows)/seconds)
        if "animation_schedule_us" in native:
            fields["animation_schedule_us"] = native["animation_schedule_us"]["mean"]
        if row["mode"] == "4a-online":
            fields.update(creature_bytes_s=sum(w["creature_period_sent_bytes"] for w in windows)/seconds,
                          creature_snapshots_s=sum(w["creature_period_sent_messages"] for w in windows)/seconds,
                          creature_sim_ms_tick=statistics.mean(w["creature_sim_mean_ms"] for w in windows))
        if "behavior" in row:
            fields["ai_step_elapsed_us"] = statistics.mean(w["ai_step_mean_us"] for w in windows)
        groups.setdefault(name, []).append(fields)
    return {group: {key: statistics.median(entry[key] for entry in values) for key in values[0]}
            for group, values in groups.items()}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("server", "client", "output"):
        parser.add_argument(f"--{name}", required=True, type=Path)
    parser.add_argument("--old-server", type=Path, help="Optional historical zero-creature network control")
    parser.add_argument("--old-client", type=Path)
    parser.add_argument("--assets", type=Path, default=Path("assets"))
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--frames", type=int, default=3600)
    parser.add_argument("--counts", type=int, nargs="+", default=[0, 1, 4, 12], choices=[0, 1, 4, 12])
    parser.add_argument("--behaviors", nargs="+", choices=["demo", "reactive", "navigation"])
    parser.add_argument("--creature-manifest")
    parser.add_argument("--mixed-macroquad", type=Path, help="Also render a second Bevy and a legacy v1 client")
    parser.add_argument("--reactive-server", type=Path, help="Use the historical 4B.1 server for paired reactive trials")
    args = parser.parse_args()
    if args.mixed_macroquad and (not args.creature_manifest or args.old_server):
        parser.error("Mixed peers require a manifest and the current server/client")
    if bool(args.old_server) != bool(args.old_client):
        parser.error("Both control binaries are required together")
    args.output.mkdir(parents=True, exist_ok=True)
    host = hardware()
    host["started_utc"] = datetime.now(timezone.utc).isoformat()
    host["os_release"] = platform.version()
    cases = [(n, False, mode) for n in args.counts for mode in (args.behaviors or [None])]
    if args.old_server:
        cases.insert(0, (0, True, None))
    rows = []
    for i in range(1, args.repeats+1):
        for count, old, mode in (cases if i % 2 else reversed(cases)):
            rows.append(trial(args, i, count, old, host, mode))
    helpers["write_json"](args.output / "summary.json", summarize(rows))
