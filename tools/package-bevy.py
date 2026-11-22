"""Package Windows milestone 3; reuse milestone 2 notices and launchers.

Initialize tools/env-windows.ps1 first. --prepare-only needs no executables.
Cargo is always offline: only missing licence texts use explicit upstream URLs.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import urllib.error
import urllib.request
from zipfile import ZIP_DEFLATED, ZipFile

SPEC = importlib.util.spec_from_file_location("package_mp", Path(__file__).with_name("package-mp.py"))
mp = importlib.util.module_from_spec(SPEC)
old_bytecode_setting = sys.dont_write_bytecode
try:
    sys.dont_write_bytecode = True
    SPEC.loader.exec_module(mp)
finally:
    sys.dont_write_bytecode = old_bytecode_setting


def cargo(repo, *args):
    return subprocess.check_output(["cargo", args[0], "--offline", *args[1:]], cwd=repo,
                                   text=True, encoding="utf-8")


mp.cargo = cargo
write = mp.write


def fetch(url):
    request = urllib.request.Request(url, headers={"User-Agent": "PrimalVoxel-licence-packager"})
    return urllib.request.urlopen(request, timeout=30).read()


def upstream_licences(package, out):
    """Resolve omitted workspace licences against the published crate's VCS SHA."""
    source = Path(package["manifest_path"]).parent
    info = source / ".cargo_vcs_info.json"
    repository = (package.get("repository") or "").rstrip("/").removesuffix(".git")
    if not info.is_file() or not repository.startswith("https://github.com/"):
        raise RuntimeError(f"No pinned upstream licence source: {package['name']}")
    revision = json.loads(info.read_text())["git"]["sha1"]
    provenance = []
    # Known upstream workspace-root naming conventions. Only exact revision URLs.
    for name in ("LICENSE-MIT", "LICENSE-APACHE", "LICENSE", "LICENSE.txt", "LICENSE.md",
                 "LICENSE.MIT", "LICENSE.APACHE", "LICENSE-MIT-0", "NOTICE"):
        url = f"https://raw.githubusercontent.com/{repository[19:]}/{revision}/{name}"
        try:
            content = fetch(url)
        except urllib.error.HTTPError as error:
            if error.code == 404:
                continue
            raise
        if len(content) < 100:
            raise RuntimeError(f"Unexpected licence response: {url}")
        (out / name).write_bytes(content)
        provenance.append(f"{url}\nSHA-256 {hashlib.sha256(content).hexdigest()}")
    if not provenance and package["name"] == "constgebra" and package["license"] == "MIT OR Apache-2.0":
        # This exact release declares both licences but ships no separate text.
        # Select its Apache-2.0 alternative and include the canonical text.
        url = "https://www.apache.org/licenses/LICENSE-2.0.txt"
        content = fetch(url)
        (out / "LICENSE-APACHE").write_bytes(content)
        provenance.append(f"{repository}/tree/{revision}\nCargo.toml: MIT OR Apache-2.0; selected Apache-2.0.\n{url}\nSHA-256 {hashlib.sha256(content).hexdigest()}")
    if not provenance:
        raise RuntimeError(f"No licence at published revision: {package['name']} {revision}")
    write(out / "PROVENANCE.txt", "Licences du dépôt officiel au commit du paquet publié.\n"
          + "\n".join(provenance) + "\n")


def native_headers(destination):
    """Notices for generated Vulkan/SPIR-V definitions; no driver is redistributed."""
    for name, repository, tag in (
        ("Vulkan-Headers-1.3.281", "KhronosGroup/Vulkan-Headers", "v1.3.281"),
        ("SPIRV-Headers-1.4.341.0", "KhronosGroup/SPIRV-Headers", "vulkan-sdk-1.4.341.0"),
    ):
        out = destination / "LICENCES" / name
        if (out / "PROVENANCE.txt").is_file():
            continue
        revision = json.loads(fetch(f"https://api.github.com/repos/{repository}/commits/{tag}"))["sha"]
        out.mkdir(parents=True, exist_ok=True)
        provenance = []
        for filename in ("LICENSE", "LICENSE.txt", "LICENSE.md", "LICENSES/MIT.txt", "LICENSES/Apache-2.0.txt", "LICENSES/CC-BY-4.0.txt"):
            url = f"https://raw.githubusercontent.com/{repository}/{revision}/{filename}"
            try:
                content = fetch(url)
            except urllib.error.HTTPError as error:
                if error.code == 404:
                    continue
                raise
            (out / filename).parent.mkdir(parents=True, exist_ok=True)
            (out / filename).write_bytes(content)
            provenance.append(f"{url}\nSHA-256 {hashlib.sha256(content).hexdigest()}")
        if not provenance:
            raise RuntimeError(f"Missing native notice: {name}")
        write(out / "PROVENANCE.txt", f"Version {tag}; commit {revision}.\n"
              + "\n".join(provenance) + "\n")


def collect_notices(repo, destination, baseline):
    mp.collect_notices(repo, destination, baseline)
    metadata = json.loads(cargo(repo, "metadata", "--locked", "--format-version", "1",
                                "--filter-platform", mp.TARGET))
    tree = cargo(repo, "tree", "--locked", "--target", mp.TARGET, "-p", "primal-client",
                 "-p", "primal-server", "-p", "primal-client-bevy", "-e", "normal,build",
                 "--prefix", "none", "--format", "{p}")
    selected = set(re.findall(r"^([\w-]+) v([\w.+-]+)", tree, flags=re.M))
    packages = sorted((p for p in metadata["packages"] if p["source"] and
                       (p["name"], p["version"]) in selected), key=lambda p: (p["name"], p["version"]))
    licences = destination / "LICENCES"
    for package in packages:
        source = Path(package["manifest_path"]).parent
        out = licences / "crates" / f"{package['name']}-{package['version']}"
        out.mkdir(parents=True, exist_ok=True)
        files = [p for p in source.rglob("*") if p.is_file() and mp.LICENSE_NAME.match(p.name)]
        for path in files:
            target = out / path.relative_to(source)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, target)
        if not files and not (out / "MIT-TEXTE-STANDARD.txt").is_file() and not any(p.is_file() and mp.LICENSE_NAME.match(p.name) for p in out.rglob("*")):
            upstream_licences(package, out)
        shutil.copy2(package["manifest_path"], out / "Cargo.toml")
        declaration = {k: package.get(k) for k in ("name", "version", "license", "authors", "repository", "source")}
        write(out / "DECLARATION.json", json.dumps(declaration, ensure_ascii=False, indent=2) + "\n")
    tables = next(Path(p["manifest_path"]).parent for p in packages if p["name"] == "bevy_core_pipeline")
    lut_dir = licences / "Bevy-tonemapping"
    lut_dir.mkdir(exist_ok=True)
    shutil.copy2(tables / "src/tonemapping/luts/info.txt", lut_dir / "PROVENANCE.txt")
    for name in ("LICENSE-MIT", "LICENSE-APACHE"):
        base = "https://raw.githubusercontent.com/h3r2tic/tony-mc-mapface/909e51c8a74251fd828770248476cb084081e08c/"
        notice = lut_dir / ("Tony-" + name)
        if not notice.exists():
            notice.write_bytes(fetch(base + name))
    native_headers(destination)
    write(licences / "cargo-tree-windows.txt", tree)
    write(licences / "PACKAGES.json", json.dumps(
        [{k: p.get(k) for k in ("name", "version", "license", "authors", "repository", "source")} for p in packages],
        ensure_ascii=False, indent=2) + "\n")
    write(licences / "INDEX.txt", "NOTICES TIERCES — PRIMAL VOXEL 3 WINDOWS\n"
          "Versions Windows GNU x64 des deux clients et du serveur; dépendances normal,build.\n"
          "Les textes proviennent des paquets verrouillés ou de leur commit publié exact.\n"
          "Inclut Bevy, WGPU, Naga, Ash, Renet, Rust, MinGW, ProggyClean, Vulkan/SPIR-V.\n"
          "Aucun chargeur Vulkan, pilote GPU ni runtime Microsoft n'est redistribué.\n"
          "Les licences concernent chaque composant et non le code original du projet.\n\n"
          + f"{len(packages)} paquets tiers\n"
          + "\n".join(f"{p['name']}-{p['version']} — {p['license']}" for p in packages) + "\n")
    return len(packages)


NOTICE = """PRIMAL VOXEL 3D — WINDOWS x64

Décompressez entièrement ce dossier dans un emplacement inscriptible.
Inspecter-Wyverne.cmd ouvre la démonstration locale de la wyverne animée (voir docs/CREATURES_3D.md).
Jouer-Foret.cmd ouvre la forêt jouable avec Bevy et son serveur autoritaire local.
Fermez le client pour sauvegarder et arrêter le serveur. Le monde est dans worlds/foret.pvx.regions.
Serveur-Foret.cmd lance seulement le serveur de forêt (port 47621). Les deux clients peuvent le rejoindre.
Visiter-Foret.cmd ouvre la caméra libre de visite. Jouer-Bevy.cmd ouvre la vallée historique en caméra libre.
PrimalVoxel-Bevy.exe est le même client; le lanceur fixe le dossier courant.
Jouer.cmd ouvre le client historique Macroquad, avec menu Solo / Héberger / Rejoindre.
PrimalVoxel.exe et primal-server.exe conservent le multijoueur LAN du jalon 2.
Consultez LISEZMOI-MULTIJOUEUR.txt pour ce client, le réseau et les sauvegardes.

Aucun Cargo, Rust, installation du SDK Vulkan ou téléchargement d'actifs n'est
nécessaire. Le pilote graphique Windows doit fournir Vulkan; mettez à jour le
pilote officiel du GPU si le client ne trouve aucun adaptateur compatible.
Le lanceur choisit Vulkan pour ne pas dépendre de dxcompiler.dll/dxil.dll.
Les shaders Bevy sont intégrés à l'exécutable. Les mondes restent locaux au PC.
Le guide et les mesures sont dans docs/FORET_3C.md et docs/PERFORMANCE_3C.md.
Retrouvez la vidéo et les captures de la wyverne dans benchmarks/results/3d/,
et les vues de la forêt de référence dans benchmarks/results/3c/.

Pour rejoindre le serveur local avec Bevy, démarrer le serveur puis lancer
Jouer-Bevy.cmd --connect 127.0.0.1:47620 --identity joueur-bevy.identity
Le même serveur accepte Macroquad et Bevy. Un clic capture la souris, Échap la
libère. WASD physiques, Espace, Maj et F contrôlent déplacement, saut, sprint
et vol. Un clic gauche détruit le bloc visé, le droit pose le matériau 1 à 7.
R reconnecte la même identité et le client retente une connexion interrompue.
Pour un serveur à cadence personnalisée, ajouter --input-hz N dans Bevy,
ou --tick-rate N dans Macroquad. Par défaut les deux envoient à 60 Hz.
Ne remplacez pas un monde existant pour tester le nouveau rendu.

LICENCES contient les textes exacts des dépendances des trois exécutables.
SHA256SUMS.txt contient leurs empreintes et celles des fichiers distribués.
"""


def pe_imports(path):
    """Read PE32+ imports without a compiler or an extra Python dependency."""
    data = path.read_bytes()
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[:2] != b"MZ" or data[pe:pe + 4] != b"PE\0\0":
        raise ValueError(f"Not a Windows PE executable: {path}")
    machine, sections = struct.unpack_from("<HH", data, pe + 4)
    optional_size = struct.unpack_from("<H", data, pe + 20)[0]
    optional = pe + 24
    if machine != 0x8664 or struct.unpack_from("<H", data, optional)[0] != 0x20B:
        raise ValueError(f"Not a Windows x64 executable: {path}")
    section_table = optional + optional_size

    def offset(rva):
        for index in range(sections):
            entry = section_table + index * 40
            virtual_size, address, size, pointer = struct.unpack_from("<IIII", data, entry + 8)
            if address <= rva < address + max(size, virtual_size):
                return pointer + rva - address
        raise ValueError(f"Invalid PE RVA {rva}: {path}")

    imports_rva = struct.unpack_from("<I", data, optional + 120)[0]
    result = []
    if imports_rva:
        descriptor = offset(imports_rva)
        while any(data[descriptor:descriptor + 20]):
            start = offset(struct.unpack_from("<I", data, descriptor + 12)[0])
            result.append(data[start:data.index(b"\0", start)].decode("ascii"))
            descriptor += 20
    return sorted(result, key=str.lower)


def main():
    repo = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=repo.parent / "PrimalVoxel-3D-Windows-x64")
    parser.add_argument("--baseline-notices", type=Path, default=repo.parent / "PrimalVoxel-1C-Windows-x64/LICENCES")
    parser.add_argument("--client", type=Path, help="Validated Macroquad client")
    parser.add_argument("--bevy-client", type=Path, help="Validated Bevy client")
    parser.add_argument("--server", type=Path)
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    binaries = {"PrimalVoxel.exe": args.client, "PrimalVoxel-Bevy.exe": args.bevy_client,
                "primal-server.exe": args.server}
    if not args.prepare_only and not all(binaries.values()):
        parser.error("Provide --client, --bevy-client and --server, or --prepare-only")
    imports = {}
    for name, source in binaries.items():
        if source:
            imports[name] = pe_imports(source)
            # All dependencies must be Windows system DLLs; no compiler DLLs.
            missing = [dll for dll in imports[name] if not dll.lower().startswith(("api-ms-", "ext-ms-"))
                       and not (Path("C:/Windows/System32") / dll).is_file()]
            if missing:
                raise RuntimeError(f"External runtime dependencies in {name}: {missing}")
    args.out.mkdir(parents=True, exist_ok=True)
    count = collect_notices(repo, args.out, args.baseline_notices)
    shutil.copytree(repo / "tools/launch", args.out / "tools/launch", dirs_exist_ok=True)
    for name, script in (("Jouer", "menu"), ("Solo", "solo"), ("Heberger", "host"),
                         ("Rejoindre", "join"), ("Serveur", "server"), ("Jouer-Bevy", "bevy")):
        write(args.out / f"{name}.cmd", '@echo off\n'
              f'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\\launch\\{script}.ps1" %*\n'
              'if errorlevel 1 pause\n')
    def launcher(name, script, fixed):
        write(args.out / (name + ".cmd"), '@echo off\n'
              f'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\\launch\\{script}.ps1" {fixed} %*\n'
              'if errorlevel 1 pause\n')
    launcher("Jouer-Foret", "host", "-Bevy -Config serveur-foret.json -Port 47621")
    launcher("Serveur-Foret", "server", "-Config serveur-foret.json")
    launcher("Inspecter-Wyverne", "bevy", "--preset forest-modern --creatures 1 --creature-state demo --view 0.5,27.8,0.5,-1.66,0.02")
    launcher("Visiter-Foret", "bevy", "--preset forest-modern --view 0.5,27.8,0.5,-1.66,0.08")
    config = {"world": "worlds/host.pvx", "bind": "0.0.0.0:47620", "seed": 20261120,
              "tick_hz": 60, "max_players": 8}
    if not (args.out / "server.json").exists():
        write(args.out / "server.json", json.dumps(config, indent=2) + "\n")
    write(args.out / "server.example.json", json.dumps(config, indent=2) + "\n")
    forest = dict(config, world="worlds/foret.pvx", bind="0.0.0.0:47621", preset="forest-modern")
    write(args.out / "serveur-foret.example.json", json.dumps(forest, indent=2) + "\n")
    if not (args.out / "serveur-foret.json").exists():
        shutil.copy2(args.out / "serveur-foret.example.json", args.out / "serveur-foret.json")
    write(args.out / "LISEZMOI.txt", NOTICE)
    write(args.out / "LISEZMOI-MULTIJOUEUR.txt", mp.NOTICE)
    if args.prepare_only:
        print(f"Prepared {count} exact Windows package notices and launchers: {args.out}")
        return
    for name, source in binaries.items():
        shutil.copy2(source, args.out / name)
    write(args.out / "WINDOWS-IMPORTS.json", json.dumps(imports, indent=2) + "\n")
    managed = [args.out / name for name in (*binaries, "LISEZMOI.txt", "LISEZMOI-MULTIJOUEUR.txt",
                                            "server.example.json", "WINDOWS-IMPORTS.json")]
    managed.append(args.out / "serveur-foret.example.json")
    extras = [repo / "docs" / name for name in ("FORET_3C.md", "PERFORMANCE_3C.md", "MULTIJOUEUR_2.md")]
    extras += [repo / "tools" / name for name in ("benchmark-forest.py", "test-mixed-renderers.py")]
    extras += sorted(f for f in (repo / "benchmarks/results/3c").rglob("*") if f.is_file())
    extras += [repo / "docs" / name for name in ("CREATURES_3D.md", "WYVERN_ASSET.md")]
    extras += [repo / "tools" / name for name in ("generate-wyvern.py", "validate-wyvern.py", "benchmark-creatures.py", "test-creature-import.py", "check-creature-motion.py")]
    extras += sorted(f for f in (repo / "assets/creatures").iterdir() if f.is_file())
    extras += sorted(f for f in (repo / "benchmarks/results/3d").rglob("*") if f.is_file())
    for source in extras:
        target = args.out / source.relative_to(repo)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, target)
        managed.append(target)
    managed += list(args.out.glob("*.cmd"))
    managed += [p for folder in ("LICENCES", "tools/launch") for p in (args.out / folder).rglob("*") if p.is_file()]
    hashes = [f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(args.out).as_posix()}" for p in sorted(managed)]
    # server.json in the archive is always the default, never a played configuration.
    hashes.append(f"{hashlib.sha256((args.out / 'server.example.json').read_bytes()).hexdigest()}  server.json")
    hashes.append(f"{hashlib.sha256((args.out / 'serveur-foret.example.json').read_bytes()).hexdigest()}  serveur-foret.json")
    write(args.out / "SHA256SUMS.txt", "\n".join(hashes) + "\n")
    archive = args.out.with_suffix(".zip")
    with ZipFile(archive, "w", ZIP_DEFLATED, strict_timestamps=False) as bundle:
        for file in [*sorted(managed), args.out / "SHA256SUMS.txt"]:
            bundle.write(file, file.relative_to(args.out).as_posix())
        bundle.writestr("serveur-foret.json", (args.out / "serveur-foret.example.json").read_bytes())
        bundle.writestr("server.json", (args.out / "server.example.json").read_bytes())
    with ZipFile(archive) as bundle:
        if bundle.testzip() is not None:
            raise ValueError("Archive CRC failure")
        for line in hashes:
            expected, name = line.split("  ", 1)
            if hashlib.sha256(bundle.read(name)).hexdigest() != expected:
                raise ValueError(f"Archive SHA-256 failure: {name}")
    print(f"Packaged 3 Windows executables, {count} dependency notices; CRC/SHA-256 verified: {archive}")


if __name__ == "__main__":
    main()
