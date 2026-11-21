#!/usr/bin/env python3
"""Real server + separate process clients, with deterministic UDP impairment.

Build the server normally and the probe with `cargo build --manifest-path
tools/net-probe/Cargo.toml`. No protocol is reimplemented in Python. Reports and
world files remain in --output for independent inspection; existing output is
never deleted. Run --clients 4 for two simultaneous distant player groups.
"""
import argparse
import heapq
import json
import math
from pathlib import Path
import random
import socket
import subprocess
import threading
import time


class JsonProcess:
    def __init__(self, command, log):
        self.log = open(log, "w", encoding="utf-8")
        self.process = subprocess.Popen([str(x) for x in command], stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                        text=True, encoding="utf-8", bufsize=1)
        self.records = []
        self.condition = threading.Condition()
        self.sequence = 0
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()

    def _read(self):
        for line in self.process.stdout:
            self.log.write(line)
            self.log.flush()
            try:
                value = json.loads(line)
            except json.JSONDecodeError:
                continue
            with self.condition:
                self.records.append(value)
                self.condition.notify_all()
        with self.condition:
            self.condition.notify_all()

    def write(self, line):
        self.process.stdin.write(line + "\n")
        self.process.stdin.flush()

    def wait(self, predicate, timeout=20, after=0):
        deadline = time.monotonic() + timeout
        with self.condition:
            while True:
                for record in self.records[after:]:
                    if predicate(record):
                        return record
                if self.process.poll() is not None:
                    raise AssertionError(f"process exited {self.process.returncode}: {self.records[-4:]}")
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise AssertionError(f"timeout; recent events: {self.records[-4:]}")
                self.condition.wait(min(remaining, .2))

    def ask(self, op, **arguments):
        self.sequence += 1
        request_id = self.sequence
        self.write(json.dumps(dict(request_id=request_id, op=op, **arguments)))
        response = self.wait(lambda r: r.get("request_id") == request_id)
        if "error" in response:
            raise AssertionError(response["error"])
        return response["result"]

    def close(self, graceful=None):
        if self.process.poll() is None:
            if graceful:
                self.write(graceful)
                try:
                    self.process.wait(25)
                except subprocess.TimeoutExpired:
                    self.process.kill()
                    raise AssertionError("graceful shutdown timed out")
            else:
                self.process.kill()
                self.process.wait(5)
        self.reader.join(2)
        if not self.log.closed:
            self.log.close()


class UdpRelay:
    """One client per relay: drop/delay real encrypted datagrams both ways."""
    def __init__(self, server, loss, delay_ms, seed):
        self.server = server
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.socket.bind(("127.0.0.1", 0))
        self.socket.settimeout(.002)
        self.address = self.socket.getsockname()
        self.client = None
        self.loss, self.delay = loss, delay_ms / 1000
        self.random = random.Random(seed)
        self.queue = []
        self.sequence = 0
        self.received = self.dropped = self.forwarded = 0
        self.started = time.monotonic()
        self.finished = None
        self.traffic = {direction: dict(received_datagrams=0, received_bytes=0,
                                      forwarded_datagrams=0, forwarded_bytes=0,
                                      dropped_datagrams=0, dropped_bytes=0)
                        for direction in ("client_to_server", "server_to_client")}
        self.running = True
        self.thread = threading.Thread(target=self._run, daemon=True)
        self.thread.start()

    def _run(self):
        while self.running:
            try:
                data, source = self.socket.recvfrom(65535)
                self.received += 1
                if source == self.server:
                    target = self.client
                    direction = "server_to_client"
                else:
                    self.client = source
                    target = self.server
                    direction = "client_to_server"
                counters = self.traffic[direction]
                counters["received_datagrams"] += 1
                counters["received_bytes"] += len(data)
                if self.random.random() < self.loss:
                    self.dropped += 1
                    counters["dropped_datagrams"] += 1
                    counters["dropped_bytes"] += len(data)
                elif target is not None:
                    self.sequence += 1
                    due = time.monotonic() + self.random.uniform(0, self.delay)
                    heapq.heappush(self.queue, (due, self.sequence, data, target, direction))
            except socket.timeout:
                pass
            except OSError:
                break
            while self.queue and self.queue[0][0] <= time.monotonic():
                _, _, data, target, direction = heapq.heappop(self.queue)
                self.socket.sendto(data, target)
                self.forwarded += 1
                self.traffic[direction]["forwarded_datagrams"] += 1
                self.traffic[direction]["forwarded_bytes"] += len(data)

    def close(self):
        self.running = False
        self.thread.join(2)
        self.socket.close()
        self.finished = self.finished or time.monotonic()

    def metrics(self):
        duration = max((self.finished or time.monotonic()) - self.started, .000001)
        return dict(duration_s=duration, udp_payload_only=True,
                    directions={direction: dict(counters,
                        received_bytes_per_s=counters["received_bytes"] / duration,
                        received_datagrams_per_s=counters["received_datagrams"] / duration,
                        forwarded_bytes_per_s=counters["forwarded_bytes"] / duration,
                        forwarded_datagrams_per_s=counters["forwarded_datagrams"] / duration)
                        for direction, counters in self.traffic.items()})


def eventually(function, predicate=lambda x: bool(x), timeout=30):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        try:
            last = function()
            if predicate(last):
                return last
        except AssertionError as error:
            last = str(error)
        time.sleep(.08)
    raise AssertionError(f"convergence deadline exceeded: {last}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server", type=Path, required=True)
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--clients", type=int, choices=[2, 4], default=2)
    parser.add_argument("--loss", type=float, default=0)
    parser.add_argument("--delay-ms", type=float, default=0)
    parser.add_argument("--seed", type=int, default=20261121)
    parser.add_argument("--terrain-only", action="store_true", help="run terrain replication and 4 km streaming, skipping identity persistence/reboot")
    args = parser.parse_args()
    if not 0 <= args.loss <= .4 or not 0 <= args.delay_ms <= 1000:
        parser.error("loss must be 0..0.4 and delay-ms 0..1000")
    args.output.mkdir(parents=True, exist_ok=False)
    output = args.output.resolve()
    free = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    free.bind(("127.0.0.1", 0))
    address = free.getsockname()
    free.close()
    endpoint = f"{address[0]}:{address[1]}"
    world = output / "world.pvx"
    processes, relays, checks = [], [], []
    server = None
    generation = 0

    def record(name, **details):
        checks.append(dict(check=name, **details))
        print(json.dumps(checks[-1]), flush=True)

    def start_server():
        nonlocal generation
        generation += 1
        process = JsonProcess([args.server.resolve(), "--world", world, "--bind", endpoint], output / f"server-{generation}.jsonl")
        processes.append(process)
        process.wait(lambda r: r.get("event") == "started")
        return process

    def start_client(index, attempt=0):
        relay = UdpRelay(address, args.loss, args.delay_ms, args.seed + index * 100 + attempt)
        relay.label = dict(client=index, generation=generation, attempt=attempt)
        relays.append(relay)
        process = JsonProcess([args.probe.resolve(), f"127.0.0.1:{relay.address[1]}", output / f"identity-{index}.bin"], output / f"client-{index}-{generation}-{attempt}.jsonl")
        processes.append(process)
        welcome = process.wait(lambda r: r.get("event") == "welcome", timeout=45)
        eventually(lambda: process.ask("status"), lambda r: any(p["id"] == welcome["id"] for p in r["players"]))
        return process, welcome["id"]

    teleports = 0
    def teleport(client, player_id, position):
        nonlocal teleports
        teleports += 1
        # The operator console and UDP are independent. Observe the newer neutral
        # command at the authority before issuing the console move.
        marker = .0123 * teleports
        client.ask("input", movement=[0, 0, 0], flying=True, yaw=marker)
        eventually(lambda: actor(client, player_id),
                   lambda state: state["flying"] and abs(state["yaw"] - marker) < .00001)
        server.write("teleport " + str(player_id) + " " + " ".join(map(str, position)))
        eventually(lambda: client.ask("status"), lambda r: any(p["id"] == player_id and math.dist(p["position"], position) < .15 for p in r["players"]))

    def block(client, position, material):
        return eventually(lambda: client.ask("block", pos=position), lambda r: r["material"] == material)

    def absent(client, position):
        return eventually(lambda: client.ask("block", pos=position), lambda r: r["material"] is None and r["revision"] is None)

    def actor(client, player_id):
        return next(p for p in client.ask("status")["players"] if p["id"] == player_id)

    def finish():
        for client in clients:
            client.ask("quit")
            client.process.wait(10)
        server.close("stop")
        assert server.process.returncode == 0
        if args.loss:
            assert sum(r.dropped for r in relays) > 0, "impairment did not drop any packet"

    def outcome(client, seq):
        return client.wait(lambda r: r.get("event") == "outcome" and r["seq"] == seq)

    def edit(client, position, material, accepted=True, revision=None):
        arguments = dict(pos=position, material=material)
        if revision is not None:
            arguments["revision"] = revision
        response = client.ask("edit", **arguments)
        result = outcome(client, response["seq"])
        assert result["accepted"] == accepted, result
        if not accepted:
            assert result["reason"], result
        return response

    clients, ids, saved = [], [], []
    try:
        server = start_server()
        for index in range(args.clients):
            client, player_id = start_client(index)
            clients.append(client)
            ids.append(player_id)
        for client in clients:
            eventually(lambda: client.ask("status"), lambda r: set(ids).issubset({p["id"] for p in r["players"]}))
        record("mutual_appearance", clients=args.clients, distinct_pids=[c.process.pid for c in clients])

        # Separate the second pair before digging under the first pair's spawn.
        # Otherwise the server correctly rejects replacing a block inside a bystander.
        for index in range(2, args.clients):
            teleport(clients[index], ids[index], [2048.5, 100., -2047.5])

        for group in range(args.clients // 2):
            a, b = clients[group * 2:group * 2 + 2]
            ia, ib = ids[group * 2:group * 2 + 2]
            x, z = group * 2048, group * -2048
            teleport(a, ia, [x + .5, 100, z + .5])
            top = eventually(lambda: a.ask("surface", x=x, z=z))["pos"][1]
            position = [x + .5, top + 3., z + .5]
            teleport(a, ia, position)
            teleport(b, ib, position)
            a.ask("input", movement=[1, 0, 0], flying=True)
            eventually(lambda: b.ask("status"), lambda r: any(p["id"] == ia and p["position"][0] > position[0] + .5 for p in r["players"]))
            teleport(a, ia, position)
            record("movement_observed", group=group)
            for client in (a, b):
                client.ask("input", movement=[0, 0, 0], flying=True, yaw=0, pitch=-math.pi / 2)
            ray = eventually(lambda: a.ask("ray"))
            target, original = ray["block"], ray["material"]
            block(b, target, original)
            original_revision = a.ask("block", pos=target)["revision"]
            edit(a, target, 0)
            block(b, target, 0)
            record("destruction_replicated", group=group, target=target)
            # Same origin/direction now hits the supporting voxel below the hole.
            place_ray = eventually(lambda: b.ask("ray"), lambda r: r["adjacent"] == target)
            assert place_ray["adjacent"] == target
            edit(b, target, original)
            block(a, target, original)
            record("placement_replicated", group=group)
            edit(a, target, 0, accepted=False, revision=original_revision)
            edit(a, [target[0] + 1000, target[1], target[2]], 0, accepted=False, revision=0)
            record("stale_and_out_of_reach_rejected", group=group)
            revision = a.ask("block", pos=target)["revision"]
            block(b, target, original)
            requests = [c.ask("edit", pos=target, material=0, revision=revision) for c in (a, b)]
            results = [outcome(c, r["seq"]) for c, r in zip((a, b), requests)]
            assert sum(r["accepted"] for r in results) == 1, results
            for client in (a, b):
                state = block(client, target, 0)
                assert state["revision"] == revision + 1, state
            record("concurrent_edit_single_winner", group=group, outcomes=results)
            edit(b, target, original)
            block(a, target, original)
            saved.append((target, original, position))

            # A real split is required even in the two-client route. Snapshots
            # must be sent/forgotten according to each player's own centre.
            distant_x = x + 4096
            teleport(b, ib, [distant_x + .5, 100, z + .5])
            distant = eventually(lambda: b.ask("surface", x=distant_x, z=z))
            distant_target = distant["pos"]
            distant_position = [distant_x + .5, distant_target[1] + 3., z + .5]
            teleport(b, ib, distant_position)
            block(a, target, original)
            absent(a, distant_target)
            block(b, distant_target, distant["material"])
            absent(b, target)
            record("separate_streaming_centres", group=group, separation_m=4096,
                   home=target, distant=distant_target, home_owner=ia, distant_owner=ib)
            teleport(a, ia, distant_position)
            teleport(b, ib, position)
            block(a, distant_target, distant["material"])
            absent(a, target)
            block(b, target, original)
            absent(b, distant_target)
            record("separate_streaming_centres_swapped", group=group,
                   home_owner=ib, distant_owner=ia)
            teleport(a, ia, position)
            block(a, target, original)
            block(b, target, original)
            absent(a, distant_target)
            absent(b, distant_target)
            record("separate_streaming_return", group=group)

        if args.terrain_only:
            finish()
            report = dict(passed=True, terrain_only=True, checks=checks)
            return

        old = ids[0]
        clients[0].ask("quit")
        clients[0].process.wait(10)
        eventually(lambda: clients[1].ask("status"), lambda r: old not in {p["id"] for p in r["players"]})
        clients[0], ids[0] = start_client(0, 1)
        assert ids[0] == old, (ids[0], old)
        teleport(clients[0], old, saved[0][2])
        block(clients[0], saved[0][0], saved[0][1])
        record("reconnect_identity_and_world", id=old)

        # The reboot below must observe the transaction queued before shutdown.
        clients[0].ask("input", yaw=0, pitch=-math.pi / 2)
        target, material, _ = saved[0]
        # Use non-default views and capture the actual authoritative snapshots.
        # No teleport or look input may precede the reboot restoration assertion.
        restored = []
        for index, client in enumerate(clients):
            yaw = .35 + index * .4
            pitch = -math.pi / 2 if index == 0 else -.25 + index * .1
            client.ask("input", movement=[0, 0, 0], flying=True, yaw=yaw, pitch=pitch)
            state = eventually(lambda: actor(client, ids[index]),
                               lambda p: abs(p["yaw"] - yaw) < .0001 and abs(p["pitch"] - pitch) < .0001 and p["flying"])
            restored.append(state)
        # Stop as soon as the real server has submitted the edit to the disk queue.
        # A fast disk can finish before the console command reaches the next tick.
        begin_queue = len(server.records)
        request = clients[0].ask("edit", pos=target, material=0,
                                 revision=clients[0].ask("block", pos=target)["revision"])
        server.wait(lambda event: event.get("event") == "save_queued" and
                    event.get("seq") == request["seq"], after=begin_queue)
        started = time.monotonic()
        server.close("stop")
        assert server.process.returncode == 0
        record("stop_after_disk_submission", shutdown_ms=(time.monotonic() - started) * 1000)
        saved[0] = (target, 0, saved[0][2])
        for client in clients:
            client.close()
        server = start_server()
        for index in range(args.clients):
            client, player_id = start_client(index, 2)
            assert player_id == ids[index]
            clients[index] = client
            initial = client.ask("status")["initial_actor"]
            expected = restored[index]
            assert initial is not None, "no initial authoritative actor"
            assert math.dist(initial["position"], expected["position"]) < .15, (initial, expected)
            assert abs(initial["yaw"] - expected["yaw"]) < .0001, (initial, expected)
            assert abs(initial["pitch"] - expected["pitch"]) < .0001, (initial, expected)
            assert initial["flying"] == expected["flying"], (initial, expected)
            record("server_reboot_restored_actor_before_teleport", client=index,
                   expected=expected, observed=initial)
        for index, client in enumerate(clients):
            player_id = ids[index]
            target, material, position = saved[index // 2]
            teleport(client, player_id, position)
            block(client, target, material)
        record("server_reboot_identity_and_edits", groups=len(saved))
        finish()
        report = dict(passed=True, checks=checks)
    except BaseException as error:
        report = dict(passed=False, error=str(error), checks=checks)
        raise
    finally:
        for process in processes:
            process.close()
        for relay in relays:
            relay.close()
        if "report" in locals():
            impaired = sum(r.dropped for r in relays)
            if report["passed"] and args.loss and impaired == 0:
                report["passed"] = False
                report["error"] = "impairment did not drop any packet"
            record("udp_traffic", dropped_datagrams=impaired,
                   received_datagrams=sum(r.received for r in relays),
                   loss=args.loss, delay_ms=args.delay_ms,
                   relays=[dict(r.label, **r.metrics()) for r in relays])
            (output / "report.json").write_text(json.dumps(report, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
