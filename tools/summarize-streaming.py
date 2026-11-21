"""Aggregate accepted streaming A/B passes, preserving one observation per run."""
import argparse
from collections import defaultdict
import csv
import gzip
import json
import math
from pathlib import Path
import re


def read_run(machine):
    base = machine.with_name("1280x720")
    manifest = json.loads(machine.read_text(encoding="utf-8-sig"))
    result = json.loads(base.with_suffix(".json").read_text())
    log = base.with_suffix(".log").read_text(encoding="utf8")
    case = machine.parents[2].name
    scenario, radius = case.rsplit("-r", 1)
    repeat, variant = machine.parents[1].name.split("-")
    stop = manifest["requested_frames"]
    warmup = result["warmup_frames"]
    assert f"SMOKE PASS frames={stop} " in log and f"STREAM scenario={scenario} " in log
    assert result["samples"] == stop - warmup and result["resolution"] == [1280, 720]
    assert result["mesher"] == "greedy" and warmup == 120
    row = dict(scenario=scenario, radius=int(radius), repeat=int(repeat), variant=variant,
               sha256=manifest["release_binary_sha256"], samples=result["samples"])
    for key in ("fps_mean", "frame_p95_ms", "frame_p99_ms", "frame_p99_9_ms"):
        row[key] = result[key]
    for line in log.splitlines():
        if line.startswith(("STREAM ", "PIPELINE ")):
            for key, value in re.findall(r"(\w+)(?:=|: )(\d+(?:\.\d+)?)", line):
                row[key] = float(value) if "." in value else int(value)
    row["graphic_missing"] = row["missing_data"] + row["missing_mesh"]
    row["process_cpu_ms"] = manifest["process_cpu_ms"]
    row["working_set_mib"] = round(manifest["peak_working_set_bytes"] / 1048576, 3)
    row["generated_per_second"] = 0
    samples = defaultdict(list)
    trace = base.with_suffix(".csv")
    compressed = trace.with_suffix(".csv.gz")
    opener = trace.open if trace.exists() else lambda: gzip.open(compressed, "rt")
    with opener() as f:
        for record in csv.DictReader(f):
            if warmup <= int(record["frame"]) < stop:
                samples[record["stage"]].append(int(record["ns"]))
    wall = result["samples"] * result["frame_mean_ms"] / 1000
    row["generated_per_second"] = round(len(samples["generate"]) / wall, 2)
    for stage in ("plan_select", "queue_dispatch", "load_queue", "mesh_queue", "region_read",
                  "generate", "remesh", "prepare_buffers", "result_wait", "publish_cpu",
                  "accept_buffers_cpu", "draw_submit_cpu", "backend_flush_upload_draw",
                  "collision_probe", "evict_save_scan", "cancel_scan", "moving_command",
                  "edit_to_mesh_ready", "edit_to_durable", "arrival_to_ground_mesh"):
        values = sorted(samples[stage])
        for q in (50, 95):
            row[f"{stage}_p{q}_ms"] = round(values[math.ceil(q / 100 * len(values)) - 1] / 1e6, 5) if values else ""
    assert row["outstanding_edits"] == 0
    if scenario == "moving-edits":
        assert row["disk_verified"] > 0
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("results", type=Path)
    args = parser.parse_args()
    rows = [read_run(p) for p in sorted((args.results / "ab").glob("*/*/greedy/*.machine.json"))]
    grouped = defaultdict(set)
    hashes = defaultdict(set)
    for r in rows:
        grouped[r["scenario"], r["radius"]].add((r["variant"], r["repeat"]))
        hashes[r["variant"]].add(r["sha256"])
    expected = {(s, 2) for s in ("walk", "sprint", "fly", "traverse", "teleport", "return", "moving-edits", "long-flight")} | {("traverse", 4), ("traverse", 8)}
    assert set(grouped) == expected and len(rows) == 60
    expected_runs = {(variant, repeat) for variant in "AB" for repeat in range(1, 4)}
    assert all(items == expected_runs for items in grouped.values())
    assert all(len(h) == 1 for h in hashes.values())
    assert hashes["A"] != hashes["B"]
    with (args.results / "runs.csv").open("w", newline="", encoding="utf8") as f:
        writer = csv.DictWriter(f, fieldnames=rows[0].keys())
        writer.writeheader()
        writer.writerows(rows)


if __name__ == "__main__":
    main()
