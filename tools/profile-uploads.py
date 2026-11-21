"""Time the two Macroquad 0.4.16 driver submissions in an isolated diagnostic build."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


def run(output):
    root = Path(__file__).resolve().parents[1]
    output.mkdir(parents=True, exist_ok=True)
    packages = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version=1", "--locked", "--offline"], cwd=root
    ))["packages"]
    package, = [p for p in packages if p["name"] == "macroquad" and p["version"] == "0.4.16"]
    with tempfile.TemporaryDirectory(prefix="primal-upload-") as directory:
        temp = Path(directory)
        # Keep the registry, the game's lockfile and the release binary unchanged.
        for folder in [".cargo", "crates", "content"]:
            shutil.copytree(root / folder, temp / folder)
        for file in ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"]:
            shutil.copy2(root / file, temp / file)
        fork = temp / "macroquad"
        shutil.copytree(Path(package["manifest_path"]).parent, fork)
        src = fork / "src/quad_gl.rs"
        source = src.read_text(encoding="utf8")
        begin = source.index("    pub fn draw(&mut self, ctx:")
        end = source.index("        self.draw_calls_count = 0;", begin)
        body = source[begin:end]
        assert body.count("            ctx.buffer_update(") == 2
        body = body.replace("        let white_texture", "        let mut upload_ns=0u128; let mut payload=0usize;\n        let white_texture", 1)
        body = body.replace("            ctx.buffer_update(", "            let started=std::time::Instant::now();\n            ctx.buffer_update(", 1)
        after = "            bindings.images[0] = dc.texture"
        assert body.count(after) == 1
        body = body.replace(after, """            upload_ns+=started.elapsed().as_nanos();
            payload+=dc.vertices_count*std::mem::size_of::<Vertex>()+dc.indices_count*2;
""" + after)
        body += """        if payload>0 { println!("UPLOAD {},{},{}",self.draw_calls_count*2,payload,upload_ns); }
"""
        src.write_text(source[:begin] + body + source[end:], encoding="utf8")
        with (temp / "Cargo.toml").open("a", encoding="utf8") as f:
            f.write('\n[patch.crates-io]\nmacroquad = { path = "macroquad" }\n')
        environment = os.environ.copy()
        environment["CARGO_TARGET_DIR"] = str(temp / "target")
        subprocess.run(["cargo", "build", "--release", "--offline", "-p", "primal-client"],
                       cwd=temp, env=environment, check=True)
        exe = temp / "target/release/primal-client.exe"
        raw = subprocess.check_output([str(exe), "--scenario", "traverse", "--frames", "840",
                                       "--metrics", str(output / "metrics.json")], text=True)
        assert "SMOKE PASS frames=840" in raw
        records = [line[7:] for line in raw.splitlines() if line.startswith("UPLOAD ")]
        assert records
        (output / "uploads.csv").write_text("calls,bytes,cpu_ns\n" + "\n".join(records) + "\n", encoding="ascii")
        (output / "run.log").write_text("\n".join(line for line in raw.splitlines() if not line.startswith("UPLOAD "))+"\n", encoding="utf8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    run(parser.parse_args().output.resolve())
