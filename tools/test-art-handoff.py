"""Exercise a full PC1/PC2 hand-off using only disposable Git repositories."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile


def run(repo, *args, env=None, success=True):
    proc = subprocess.run(args, cwd=repo, capture_output=True, text=True, encoding="utf8",
                          errors="replace", env=env)
    if (proc.returncode == 0) != success:
        raise RuntimeError(f"{args}: rc={proc.returncode}\n{proc.stdout}\n{proc.stderr}")
    return (proc.stdout if success else proc.stdout + proc.stderr).strip()


def git(repo, *args, **kwargs):
    return run(repo, "git", *args, **kwargs)


def write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n", encoding="utf8")


def queue(repo):
    return json.loads((repo / "art_pipeline/QUEUE.json").read_text(encoding="utf8"))


def submit(repo, message):
    git(repo, "add", "--all")
    git(repo, "commit", "-qm", message)


def artist_status(repo, status):
    data = queue(repo)
    order = next(order for order in data["orders"] if order["id"] == "handoff-test")
    order["status"] = status
    write(repo / "art_pipeline/QUEUE.json", data)


def clone(repo, remote, name):
    path = repo / name
    git(repo, "clone", "--no-local", remote.as_uri(), str(path), env=dict(os.environ, GIT_LFS_SKIP_SMUDGE="1"))
    git(path, "config", "--local", "user.name", "Primal Handoff Test")
    git(path, "config", "--local", "user.email", "test@localhost")
    if name == "pc1":
        folder = path / ".git" / "local-team-hooks"
        folder.mkdir()
        custom = folder / "pre-push"
        custom.write_text("#!/bin/sh\necho team-hook\n", encoding="utf8")
        git(path, "config", "--local", "core.hooksPath", ".git/local-team-hooks")
        run(path, sys.executable, "tools/install-art-hooks.py", success=False)
        assert git(path, "config", "--get", "core.hooksPath") == ".git/local-team-hooks"
        assert custom.read_text(encoding="utf8") == "#!/bin/sh\necho team-hook\n"
        git(path, "config", "--local", "--unset", "core.hooksPath")
    run(path, sys.executable, "tools/install-art-hooks.py")
    return path


def handoff(root, work, client):
    for name in ("target/debug/a", ".cargo/registry/index/cache", "tools/__pycache__/cache.pyc",
                 "source.blend1", "source.blend2", "blendcache_rig/cache", "render_tmp/a.png",
                 "export_tmp/a.glb", "private.log", "benchmarks/raw/trial.json", "game.zip",
                 "build.exe", ".env.local", "secrets/private.key"):
        git(root, "check-ignore", "--no-index", "-q", name)
    for name in ("Cargo.lock", "art_pipeline/QUEUE.json", "assets/creatures/wyvern.json",
                 "tools/blender/wyvern/build.py", "tools/blender/wyvern/source.blend",
                 "assets/creatures/wyvern_art/mesh.glb", "art_pipeline/previews/wyvern/front.png",
                 "assets/creatures/wyvern_art/textures/albedo.png"):
        git(root, "check-ignore", "--no-index", "-q", name, success=False)
    for name in ("source.blend", "assets/creatures/wyvern.glb", "assets/creatures/a/textures/t.png"):
        assert git(root, "check-attr", "filter", "--", name).endswith(": lfs"), name
    assert git(root, "check-attr", "filter", "--", "docs/figure.png").endswith(": unspecified")
    remote = work / "shared.git"
    git(work, "init", "--bare", "--initial-branch=main", str(remote))
    # No change to the production repo's branches, upstreams, or remote configuration.
    git(root, "-c", f"remote.handoff-test.url={remote.as_uri()}", "push", "handoff-test",
        "HEAD:refs/heads/main", "HEAD:refs/heads/feat/code-art-pipeline")
    pc1 = clone(work, remote, "pc1")
    pc2 = clone(work, remote, "pc2")
    model = Path("assets/creatures/wyvern.glb")
    digest = hashlib.sha256((root / model).read_bytes()).hexdigest()
    for pc in (pc1, pc2):
        assert f"oid sha256:{digest}" in (pc / model).read_text(encoding="ascii")
        git(pc, "lfs", "pull")
        assert hashlib.sha256((pc / model).read_bytes()).hexdigest() == digest
        git(pc, "lfs", "fsck")
    git(pc1, "switch", "-c", "feat/code-order-test")
    data = queue(pc1)
    order = dict(data["orders"][0], id="handoff-test", spec_path="art_pipeline/specs/handoff-test.md",
                 manifest_path="assets/creatures/handoff-test_art.json", priority=2,
                 output_path="assets/creatures/handoff-test_art/", status="TODO", notes="Local test.")
    data["orders"].append(order)
    write(pc1 / "art_pipeline/QUEUE.json", data)
    (pc1 / order["spec_path"]).write_text("Test local de transmission.\n", encoding="utf8")
    submit(pc1, "PC1 defines a new order")
    git(pc1, "switch", "main")
    git(pc1, "merge", "--ff-only", "feat/code-order-test")
    git(pc1, "push", "origin", "main")
    git(pc2, "pull", "--ff-only")
    assert order in queue(pc2)["orders"]
    branch = "art/handoff-test"
    git(pc2, "switch", "-c", branch)
    artist_status(pc2, "IN_PROGRESS")
    submit(pc2, "PC2 claims the order")
    git(pc2, "push", "-u", "origin", branch)
    # Independent LFS object: keep exactly the procedural geometry; mark only glTF metadata.
    src = (pc2 / model).read_bytes()
    size = struct.unpack_from("<I", src, 12)[0]
    gltf = json.loads(src[20:20 + size])
    gltf["asset"]["extras"] = {"temporary_handoff_fixture": True}
    chunk = json.dumps(gltf, separators=(",", ":")).encode()
    chunk += b" " * (-len(chunk) % 4)
    tail = src[20 + size:]
    asset = struct.pack("<4sIII4s", b"glTF", 2, 20 + len(chunk) + len(tail), len(chunk), b"JSON") + chunk + tail
    dest = Path(order["output_path"]) / "delivery.glb"
    (pc2 / dest).parent.mkdir(parents=True, exist_ok=True)
    (pc2 / dest).write_bytes(asset)
    definition = json.loads((pc2 / "assets/creatures/wyvern.json").read_text())
    definition["model"] = dest.relative_to("assets").as_posix()
    write(pc2 / order["manifest_path"], definition)
    (pc2 / "art_pipeline/reports/handoff-test.md").write_text("Fixture de transfert.\n", encoding="utf8")
    artist_status(pc2, "READY_FOR_INTEGRATION")
    submit(pc2, "PC2 submits its asset")
    git(pc2, "push", "origin", branch)
    # The target is still TODO after the second PC's push.
    assert queue(pc1)["orders"][-1]["status"] == "TODO"
    git(pc1, "fetch", "origin")
    art_head = f"origin/{branch}"
    run(pc1, sys.executable, "tools/check-art-branch.py", "--base", "main", "--head", art_head, "--branch", branch)
    git(pc1, "switch", "--detach", art_head, env=dict(os.environ, GIT_LFS_SKIP_SMUDGE="1"))
    expected = hashlib.sha256(asset).hexdigest()
    assert f"oid sha256:{expected}" in (pc1 / dest).read_text(encoding="ascii")
    git(pc1, "lfs", "pull", "origin")
    assert hashlib.sha256((pc1 / dest).read_bytes()).hexdigest() == expected
    run(pc1, sys.executable, "tools/validate-creature.py", "--manifest", order["manifest_path"])
    if client:
        run(pc1, sys.executable, "tools/test-creature-import.py", "--client", str(client),
            "--manifest", order["manifest_path"], "--asset-root", "assets")
    git(pc1, "switch", "main")
    git(pc1, "merge", "--ff-only", art_head)
    git(pc1, "switch", "-c", "feat/code-accept-test")
    artist_status(pc1, "INTEGRATED")
    submit(pc1, "PC1 accepts validated delivery")
    git(pc1, "switch", "main")
    git(pc1, "merge", "--ff-only", "feat/code-accept-test")
    git(pc1, "push", "origin", "main")
    # A prohibited push must fail and the server-side reference must remain unchanged.
    remote_before = git(pc2, "ls-remote", "origin", f"refs/heads/{branch}")
    git(pc2, "fetch", "origin", "main")
    (pc2 / "AGENTS.md").write_text("Outside artist scope.\n", encoding="utf8")
    submit(pc2, "Fixture for rejected push")
    rejection = git(pc2, "push", "origin", branch, success=False)
    assert "AGENTS.md" in rejection, rejection
    assert git(pc2, "ls-remote", "origin", f"refs/heads/{branch}") == remote_before
    assert queue(pc1)["orders"][-1]["status"] == "INTEGRATED"
    return {"base_lfs_sha256": digest, "delivery_lfs_sha256": expected,
            "native_import": bool(client), "prohibited_push_rejected": True, "remote_type": "local bare"}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client", type=Path, help="Optional precompiled validation client")
    opts = parser.parse_args()
    with tempfile.TemporaryDirectory() as tmp:
        print(json.dumps(handoff(Path(__file__).resolve().parents[1], Path(tmp),
                                 opts.client.resolve() if opts.client else None), indent=2))
