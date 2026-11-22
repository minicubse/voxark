"""Measure and capture the Bevy creature in the 3C Low forest. Requires psutil."""

import argparse
from dataclasses import asdict, dataclass
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import re
from runpy import run_path
import statistics
import subprocess
import tempfile
import time

import psutil
from creature_gltf import Gltf


FOREST = run_path(str(Path(__file__).with_name("benchmark-forest.py")))
VIEW = FOREST["VIEWS"]["A"]
INSPECTION_VIEWS = {"side": VIEW, "front": "-7,27.8,-10,-0.29,0.0", "back": "-7,27.8,10,-2.85,0.0"}
RESOLUTION = (1280, 720)
RADIUS = 4
SEED = 20_261_120
MANIFEST = Path("creatures/wyvern.json")
TRACE_FRAME = "set_pipeline RenderPipeline with 'clustering populate pipeline'"
DRAW_COMMAND = re.compile(r"RenderPass::(?:multi_)?draw")


@dataclass(frozen=True)
class Case:
    name: str
    count: int
    placement: str = "near"
    shadows: bool = False
    state: str = "flap"
    pose_s: float | None = None
    angle: str | None = None
    historical: bool = False


def cases_for(mode, has_baseline):
    if mode == "capture":
        return [
            Case("idle-side", 1, shadows=True, state="idleground", pose_s=0.0, angle="side"),
            Case("walk-side", 1, shadows=True, state="walk", pose_s=0.25, angle="side"),
            Case("takeoff-side", 1, shadows=True, state="takeoff", pose_s=0.5, angle="side"),
            Case("flap-side-a", 1, shadows=True, pose_s=0.0, angle="side"),
            Case("flap-side-b", 1, shadows=True, pose_s=0.4, angle="side"),
            Case("flap-front", 1, shadows=True, pose_s=0.4, angle="front"),
            Case("flap-back", 1, shadows=True, pose_s=0.4, angle="back"),
            Case("glide-side", 1, shadows=True, state="glide", pose_s=0.6, angle="side"),
            Case("land-side", 1, shadows=True, state="land", pose_s=0.5, angle="side"),
            Case("near-4-palettes", 4, shadows=True, pose_s=0.4),
            Case("near-12-shadow-off", 12, pose_s=0.4),
            Case("near-12-shadow-on", 12, shadows=True, pose_s=0.4),
            Case("far-12-shadow-on", 12, "far", True, pose_s=0.4),
            Case("offscreen-12-shadow-on", 12, "offscreen", True, pose_s=0.4),
        ]
    if mode == "probe":
        return [Case("offscreen-return", 4, shadows=True)]
    if mode == "profile":
        return [Case("0", 0)] + [Case(f"{count}-near-shadow-on", count, shadows=True) for count in (1, 4, 12)]
    cases = [Case("0", 0)]
    for placement, counts in [("near", (1, 4, 12)), ("far", (12,)), ("offscreen", (12,))]:
        for count in counts:
            cases += [
                Case(f"{count}-{placement}-shadow-off", count, placement),
                Case(f"{count}-{placement}-shadow-on", count, placement, True),
            ]
    if has_baseline:
        cases.insert(0, Case("reference-3c", 0, historical=True))
    return cases


def run_order(cases, repeats):
    for repeat in range(1, repeats + 1):
        for case in cases if repeat % 2 else reversed(cases):
            yield repeat, case


def client_command(a, case, report, capture):
    executable = a.baseline if case.historical else a.client
    argv = [
        str(executable), "--route", "fixed", "--preset", "forest-modern", "--view",
        INSPECTION_VIEWS.get(case.angle, VIEW),
        "--resolution", "x".join(map(str, RESOLUTION)), "--render-radius", str(RADIUS),
        "--frames", str(a.frames), "--report", str(report),
    ]
    if not case.historical:
        argv.extend([
            "--creatures", str(case.count), "--creature-shadows", "on" if case.shadows else "off",
        ])
        if case.count:
            argv.extend(["--creature-state", case.state, "--creature-placement", case.placement,
                         "--creature-manifest", a.creature_manifest.as_posix()])
        if a.asset_root:
            argv.extend(["--asset-root", str(a.asset_root)])
    if case.pose_s is not None:
        argv.extend(["--creature-time", str(case.pose_s), "--creature-pause", "on"])
    if a.mode == "probe":
        argv.extend(["--creature-probe", "on"])
    if a.mode == "profile":
        argv.extend(["--creature-profile", "on"])
    if capture:
        argv.extend(["--capture", str(capture)])
    return argv


def trace_window(lines):
    # Same main-view clustering boundary as 3C. Keep 20 complete intervals
    # before the last interval to avoid exit and asynchronous log flushing.
    boundaries = [i for i, line in enumerate(lines) if TRACE_FRAME in line]
    if len(boundaries) < 30:
        raise RuntimeError(f"Missing GPU frame boundaries ({len(boundaries)} found)")
    boundaries = boundaries[-22:-1]
    frames = []
    for start, end in zip(boundaries, boundaries[1:]):
        group = lines[start:end]
        stamp = None
        submit_us = []
        for line in group:
            if "Queue::submit" not in line:
                continue
            timestamp = datetime.fromisoformat(line.split()[0].replace("Z", "+00:00"))
            if line.endswith("Queue::submit"):
                stamp = timestamp
            elif stamp is not None and "returned submit index" in line:
                submit_us.append((timestamp - stamp).total_seconds() * 1e6)
                stamp = None
        frames.append({
            "draw_commands": sum(bool(DRAW_COMMAND.search(line)) for line in group),
            "submits": len(submit_us),
            "submit_cpu_us": sum(submit_us),
            "submit_call_us": submit_us,
            "buffer_writes": sum("Queue::write_buffer" in line for line in group),
        })
    if not all(frame["submits"] for frame in frames):
        raise RuntimeError("No timed queue submission in at least one selected GPU interval")
    return frames, lines[boundaries[0]:boundaries[-1]]


def sample_process(command, log, environment, timeout):
    samples = []
    start = time.perf_counter()
    with log.open("wb") as sink, subprocess.Popen(
        command, stdout=sink, stderr=subprocess.STDOUT, env=environment,
    ) as child:
        try:
            proc = psutil.Process(child.pid)
            while child.poll() is None:
                elapsed = time.perf_counter() - start
                if elapsed > timeout:
                    raise TimeoutError(f"Client ran for over {timeout} s")
                with proc.oneshot():
                    memory = proc.memory_info()
                    cpu = proc.cpu_times()
                samples.append([
                    round(elapsed, 4), memory.rss, getattr(memory, "peak_wset", None),
                    cpu.user, cpu.system,
                ])
                time.sleep(0.12)
        except psutil.NoSuchProcess:
            child.wait(timeout=5)
        except BaseException:
            child.kill()
            raise
    return child.returncode, samples


def write_json(path, data):
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(data, separators=(",", ":")) + "\n", encoding="utf8")
    temporary.replace(path)


def sha256(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def source_inventory(a):
    portable = a.client.parent / "assets"
    root = a.asset_root or (portable if portable.is_dir() else Path.cwd() / "assets")
    manifest = root / a.creature_manifest
    model = root / json.loads(manifest.read_text(encoding="utf8"))["model"]
    result = {
        "manifest": str(manifest), "manifest_sha256": sha256(manifest),
        "model": str(model), "model_sha256": sha256(model), "model_file_bytes": model.stat().st_size,
    }
    document = Gltf(model.resolve(), root.resolve()).document
    clip_views = {}
    for index, clip in enumerate(document["animations"]):
        views = set()
        for sampler in clip["samplers"]:
            for field in ("input", "output"):
                accessor = document["accessors"][sampler[field]]
                if "bufferView" in accessor:
                    views.add(accessor["bufferView"])
                if "sparse" in accessor:
                    views.update(accessor["sparse"][part]["bufferView"] for part in ("indices", "values"))
        clip_views[clip.get("name", f"#{index}")] = views
    sizes = [view["byteLength"] for view in document["bufferViews"]]
    result["clip_buffer_view_bytes"] = {name: sum(sizes[i] for i in views) for name, views in clip_views.items()}
    result["unique_clip_buffer_view_bytes"] = sum(sizes[i] for i in set().union(*clip_views.values()))
    return result


def host_inventory():
    processor = platform.processor()
    if os.name == "nt":
        import winreg
        registry_path = r"HARDWARE\DESCRIPTION\System\CentralProcessor\0"
        try:
            with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE, registry_path) as key:
                processor = winreg.QueryValueEx(key, "ProcessorNameString")[0].strip()
        except OSError:
            pass
    return {
        "platform": platform.platform(), "processor": processor,
        "logical_cpus": psutil.cpu_count(), "memory_bytes": psutil.virtual_memory().total,
    }


def validate_report(a, case, data):
    if (data["route"], tuple(data["resolution"]), data["render_radius"], data["frames"]) != (
        "fixed", RESOLUTION, RADIUS, a.frames,
    ):
        raise RuntimeError("Client report differs from the requested benchmark settings")
    if data["material"] != "forest_lit" or (data["empty_frames"] and a.mode != "probe"):
        raise RuntimeError("The benchmark did not render a stable lit forest")
    if data["backend"] != "Vulkan" or (
        not a.allow_other_adapter
        and (data["device_type"] != "IntegratedGpu" or "Radeon" not in data["adapter"])
    ):
        raise RuntimeError(f"Unexpected graphics adapter {data['adapter']} ({data['backend']})")
    if case.count:
        inventory = data.get("creature_inventory")
        motion = data.get("creature_motion")
        if not isinstance(inventory, dict) or not isinstance(motion, dict):
            raise RuntimeError("The client did not report creature inventory and motion")
        if inventory["joints_all_instances"] < case.count or inventory["skinned_primitives"] < case.count:
            raise RuntimeError("Missing creature skins or joints")
        if inventory["gpu_resident_meshes"] != inventory["unique_meshes"]:
            raise RuntimeError("Creature meshes were not all published")
    if a.mode == "profile" and not isinstance(data.get("render_passes"), dict):
        raise RuntimeError("The client did not report render-pass diagnostics")
    if len(data["forest"]["frame_ms"]) != a.frames:
        raise RuntimeError("The client did not record all timed frames")


def verify_probe(motion):
    start = motion["offscreen_start_s"]
    end = motion["offscreen_end_s"]
    observations = []
    for sample in motion["samples"]:
        actor = next((actor for actor in sample["actors"] if actor["slot"] == 0), None)
        if actor is not None:
            observations.append((sample["real_s"], sample["camera_away"], actor))
    before = [obs for obs in observations if 0.25 < obs[0] < start - 0.05 and not obs[1]]
    away = [obs for obs in observations if start + 0.3 < obs[0] < end - 0.2 and obs[1]]
    returned = [obs for obs in observations if obs[0] > end + 0.2 and not obs[1]]
    if not before or len(away) < 2 or not returned:
        raise RuntimeError("Probe did not record complete before, away and return intervals")
    before = next((obs for obs in reversed(before) if obs[2]["visible_meshes"] > 0), None)
    returned = next((obs for obs in returned if obs[2]["visible_meshes"] > 0), None)
    if before is None or returned is None or any(obs[2]["visible_meshes"] for obs in away):
        raise RuntimeError("Probe did not show the same creature leave and re-enter the main view")
    rotation_reference = away[0][2]["joints"]
    moved_joints = set()
    for _, _, actor in away[1:]:
        for name, joint in actor["joints"].items():
            if name not in rotation_reference:
                continue
            dot = sum(a * b for a, b in zip(joint["rotation"], rotation_reference[name]["rotation"], strict=True))
            if abs(dot) < 1 - 1e-7:
                moved_joints.add(name)
    checkpoints = [before, away[0], away[-1], returned]
    times = [actor["elapsed_s"] for _, _, actor in checkpoints]
    if not all(a < b for a, b in zip(times, times[1:])) or not moved_joints:
        raise RuntimeError("Probe did not detect advancing pose and clock while away")
    names = ("before", "first_away", "last_away", "returned")
    return {
        "main_view_checkpoints": {
            name: {"real_s": t, "visible_meshes": actor["visible_meshes"], "elapsed_s": actor["elapsed_s"]}
            for name, (t, _, actor) in zip(names, checkpoints, strict=True)
        },
        "joints_rotating_away": sorted(moved_joints),
    }


def trial(a, case, repeat, stem, executables, provenance):
    FOREST["idle"](executables)
    environment = dict(os.environ, WGPU_BACKEND="vulkan", RUST_LOG="error,bevy_render::renderer=info")
    if a.mode == "trace":
        environment["RUST_LOG"] += ",wgpu_core::command::render=trace,wgpu_core::device::queue=trace"
    with tempfile.TemporaryDirectory(dir=a.output) as directory:
        temporary = Path(directory)
        report, log = temporary / "report.json", temporary / "client.log"
        capture = temporary / "capture.png" if a.mode == "capture" else None
        command = client_command(a, case, report, capture)
        try:
            return_code, samples = sample_process(command, log, environment, a.timeout)
            lines = log.read_text(encoding="utf8", errors="replace").splitlines()
            errors = [line for line in lines if " ERROR " in line or "panicked at" in line]
            if return_code or errors:
                raise RuntimeError(f"Client returned {return_code}: {errors[:3] or lines[-3:]}")
            data = json.loads(report.read_text(encoding="utf8"))
            validate_report(a, case, data)
            if not samples:
                raise RuntimeError("No process samples")
            if capture:
                with capture.open("rb") as source:
                    if source.read(8) != b"\x89PNG\r\n\x1a\n":
                        raise RuntimeError("Client capture is not a PNG")
            if a.mode == "trace":
                data["wgpu_frames"], raw = trace_window(lines)
                (a.output / f"{stem}.log").write_text("\n".join(raw) + "\n", encoding="utf8")
            if a.mode == "probe":
                data["offscreen_probe"] = verify_probe(data["creature_motion"])
        except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as error:
            if log.exists():
                log.replace(a.output / f"{stem}-failed.log")
            if report.exists():
                report.replace(a.output / f"{stem}-failed.json")
            raise RuntimeError(f"{stem}: {error}") from error
        argv = command[1:]
        argv[argv.index("--report") + 1] = f"{stem}.json"
        if capture:
            capture.replace(a.output / f"{stem}.png")
            argv[argv.index("--capture") + 1] = f"{stem}.png"

    frame_ms = data["forest"]["frame_ms"]
    data.update(
        case=case.name, argv=argv, cpu_cores=data["process_cpu_ms"] / sum(frame_ms),
        rss_sample_s_bytes_peak=[sample[:3] for sample in samples],
        process_sample_fields=["elapsed_s", "rss_bytes", "peak_working_set_bytes", "user_s", "system_s"],
        process_samples=samples,
        peak_rss=max(sample[2] if sample[2] is not None else sample[1] for sample in samples),
        benchmark={
            "mode": a.mode, "repeat": repeat, "scenario": asdict(case), "forest_profile": "low",
            "seed": SEED, "executable": command[0], "host": provenance["host"],
            "client_sha256": provenance["hashes"][command[0]],
            "asset_source": provenance["asset_source"] if case.count else None,
        },
    )
    write_json(a.output / f"{stem}.json", data)
    print(f"{stem}: {data['fps_avg']:.1f} FPS, P99 {data['p99_ms']:.2f} ms", flush=True)


def numeric_medians(records):
    values = {}
    for record in records:
        for name, value in record.items():
            if type(value) in (int, float):
                values.setdefault(name, []).append(value)
    return {name: statistics.median(series) for name, series in values.items()}


def summarize(output):
    grouped = {}
    for path in sorted(output.glob("*.json")):
        if path.name == "summary.json" or path.stem.endswith("-failed"):
            continue
        data = json.loads(path.read_text(encoding="utf8"))
        if not isinstance(data,dict):
            continue
        mode = data.get("benchmark", {}).get("mode")
        if mode in ("measure", "trace", "probe", "profile"):
            grouped.setdefault(mode, {}).setdefault(data["case"], []).append((path.name, data))
    if not grouped:
        raise RuntimeError(f"No successful benchmark reports found in {output}")

    fields = ("fps_avg", "fps_1low", "p95_ms", "p99_ms", "p999_ms", "cpu_cores", "peak_rss")
    summary = {}
    for mode, cases in grouped.items():
        summary[mode] = {}
        for case, entries in cases.items():
            reports = [data for _, data in entries]
            aggregates = {
                "reports": [name for name, _ in entries],
                "median": {field: statistics.median(r[field] for r in reports) for field in fields},
                "max_peak_rss": max(r["peak_rss"] for r in reports),
                "terrain_digests": sorted({r["geometry_digest"] for r in reports}),
                "animation_schedule_us": numeric_medians(r.get("animation_schedule_us", {}) for r in reports),
                "creature_inventory": numeric_medians(r.get("creature_inventory") or {} for r in reports),
            }
            if mode == "trace":
                frames = [frame for r in reports for frame in r["wgpu_frames"]]
                aggregates["wgpu_median"] = {
                    field: statistics.median(frame[field] for frame in frames)
                    for field in ("draw_commands", "submits", "submit_cpu_us", "buffer_writes")
                }
            if mode == "profile":
                aggregates["render_pass_ms"] = numeric_medians(r["render_passes"] for r in reports)
                aggregates["gpu_timestamps_available"] = all(
                    any(name.endswith("/elapsed_gpu") for name in r["render_passes"]) for r in reports
                )
            if mode == "probe":
                aggregates["offscreen_probes"] = [r["offscreen_probe"] for r in reports]
            summary[mode][case] = aggregates
    write_json(output / "summary.json", summary)
    print(output / "summary.json")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("measure", "trace", "capture", "probe", "profile", "summarize"))
    parser.add_argument("--client", type=Path, help="3D release executable")
    parser.add_argument("--baseline", type=Path, help="Optional 3C release executable in the same forest view")
    parser.add_argument("--asset-root", type=Path, help="Override the client asset directory")
    parser.add_argument("--creature-manifest", type=Path, default=MANIFEST,
                        help="Manifest path relative to the asset root (default: creatures/wyvern.json)")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--frames", type=int, help="Timed frames per run")
    parser.add_argument("--repeats", type=int, help="Passes over the scenarios, alternating forward and reverse")
    parser.add_argument("--cases", nargs="+", help="Select case or shot names, in the supplied order")
    parser.add_argument("--timeout", type=float, default=180, help="Maximum seconds per client process")
    parser.add_argument("--allow-other-adapter", action="store_true", help="Allow Vulkan adapters other than integrated Radeon")
    parser.add_argument("--dry-run", action="store_true", help="Print the commands without launching the client")
    a = parser.parse_args()
    if a.creature_manifest.is_absolute() or ".." in a.creature_manifest.parts:
        parser.error("--creature-manifest must be relative to the asset root")
    a.output = a.output.resolve()
    if a.mode == "summarize":
        summarize(a.output)
        return
    if not a.client:
        parser.error("--client is required to run the benchmark")
    a.client = a.client.resolve()
    if a.baseline:
        a.baseline = a.baseline.resolve()
    if a.asset_root:
        a.asset_root = a.asset_root.resolve()

    default_frames = {"measure": 1100, "probe": 3600, "profile": 300}
    a.frames = a.frames if a.frames is not None else default_frames.get(a.mode, 60)
    a.repeats = a.repeats if a.repeats is not None else (3 if a.mode == "measure" else 1)
    if a.frames < 1 or a.repeats < 1 or a.timeout <= 0:
        parser.error("--frames, --repeats and --timeout must be positive")
    cases = {case.name: case for case in cases_for(a.mode, a.baseline is not None)}
    if a.cases and (set(a.cases) - cases.keys() or len(set(a.cases)) != len(a.cases)):
        parser.error("Choose distinct case names from: " + ", ".join(cases))
    selected = [cases[name] for name in a.cases or cases]

    if not a.dry_run:
        a.output.mkdir(parents=True, exist_ok=True)
        paths = {a.baseline if case.historical else a.client for case in selected}
        executables = {path.name.lower() for path in paths}
        provenance = {
            "host": host_inventory(),
            "hashes": {str(path): sha256(path) for path in paths},
            "asset_source": source_inventory(a) if any(case.count for case in selected) else None,
        }
    for repeat, case in run_order(selected, a.repeats):
        prefixes = {
            "trace": "gpu-", "probe": "probe-", "profile": "profile-",
            "measure": f"run{repeat}-", "capture": "",
        }
        prefix = prefixes[a.mode]
        if a.mode != "measure" and a.repeats > 1:
            prefix += f"run{repeat}-"
        stem = prefix + case.name
        if a.dry_run:
            capture = a.output / f"{stem}.png" if a.mode == "capture" else None
            print(subprocess.list2cmdline(client_command(a, case, a.output / f"{stem}.json", capture)))
        else:
            trial(a, case, repeat, stem, executables, provenance)


if __name__ == "__main__":
    main()
