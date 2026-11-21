"""Build a portable Windows directory from final binaries and exact Cargo notices.

Run from a shell initialized by env-windows.ps1. --prepare-only collects notices
and launchers while final binaries are still being built. Never removes saves.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import urllib.request
from zipfile import ZIP_DEFLATED, ZipFile

TARGET = "x86_64-pc-windows-gnu"
LICENSE_NAME = re.compile(r"^(LICENSE|LICENCE|COPYING|NOTICE|COPYRIGHT)([-_.]|$)", re.I)


def cargo(repo, *args):
    return subprocess.check_output(["cargo", *args], cwd=repo, text=True, encoding="utf-8")


def write(path, content):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def collect_notices(repo, destination, baseline):
    metadata = json.loads(cargo(repo, "metadata", "--locked", "--format-version", "1", "--filter-platform", TARGET))
    tree = cargo(repo, "tree", "--locked", "--target", TARGET, "-p", "primal-client", "-p", "primal-server",
                 "-e", "normal,build", "--prefix", "none", "--format", "{p}")
    selected = set(re.findall(r"^([\w-]+) v([\w.+-]+)", tree, flags=re.M))
    packages = sorted((p for p in metadata["packages"] if p["source"] and
                       (p["name"], p["version"]) in selected), key=lambda p: (p["name"], p["version"]))
    licences = destination / "LICENCES"
    licences.mkdir(parents=True, exist_ok=True)
    for runtime in ("MinGW-runtime", "ProggyClean", "Rust-runtime-1.100.0"):
        source = baseline / runtime
        if not source.is_dir():
            raise RuntimeError(f"Missing verified 1C notices: {source}")
        shutil.copytree(source, licences / runtime, dirs_exist_ok=True)
    macroquad = next(p for p in packages if p["name"] == "macroquad")
    font = Path(macroquad["manifest_path"]).parent / "src/ProggyClean.ttf"
    font_sha = hashlib.sha256(font.read_bytes()).hexdigest()
    if font_sha not in (licences / "ProggyClean/PROVENANCE.txt").read_text(encoding="utf-8"):
        raise RuntimeError("Embedded Macroquad font differs from the verified 1C notices")
    index = []
    for package in packages:
        name = f"{package['name']}-{package['version']}"
        source = Path(package["manifest_path"]).parent
        out = licences / "crates" / name
        out.mkdir(parents=True, exist_ok=True)
        files = [p for p in source.rglob("*") if p.is_file() and LICENSE_NAME.match(p.name)]
        for path in files:
            to = out / path.relative_to(source)
            to.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, to)
        if not files and package["name"] in ("renet", "renet_netcode", "renetcode"):
            # These crate archives omit the workspace-root licences. Use the
            # exact published crate's VCS revision, never a moving branch.
            revision = json.loads((source / ".cargo_vcs_info.json").read_text())["git"]["sha1"]
            urls = []
            for filename in ("LICENSE-MIT", "LICENSE-APACHE"):
                url = f"https://raw.githubusercontent.com/lucaspoffo/renet/{revision}/{filename}"
                content = urllib.request.urlopen(url, timeout=30).read()
                (out / filename).write_bytes(content)
                urls.append(f"{url}\nSHA-256 {hashlib.sha256(content).hexdigest()}")
            write(out / "PROVENANCE.txt", "Licences racine du dépôt officiel, commit du paquet publié.\n" + "\n".join(urls) + "\n")
        elif not files:
            previous = baseline / "crates" / name
            if not previous.is_dir():
                raise RuntimeError(f"No licence files or verified fallback for {name}")
            shutil.copytree(previous, out, dirs_exist_ok=True)
        declaration = {k: package.get(k) for k in ("name", "version", "license", "authors", "repository", "source")}
        shutil.copy2(package["manifest_path"], out / "Cargo.toml")
        write(out / "DECLARATION.json", json.dumps(declaration, ensure_ascii=False, indent=2) + "\n")
        index.append(f"{name} — {package['license']} — crates/{name}/")
    write(licences / "cargo-tree-windows.txt", tree)
    write(licences / "PACKAGES.json", json.dumps(
        [{k: p.get(k) for k in ("name", "version", "license", "authors", "repository", "source")} for p in packages],
        ensure_ascii=False, indent=2) + "\n")
    write(licences / "INDEX.txt", "NOTICES TIERCES — PRIMAL VOXEL 2 WINDOWS\n"
          "Inventaire cargo tree normal,build pour primal-client et primal-server, cible Windows GNU x64.\n"
          "Inclut dépendances de compilation et macros, pas les dépendances de test seules.\n"
          "Versions exactes verrouillées; licences du registre local et provenance explicite des exceptions.\n"
          "Notices police, Rust 1.100.0 et MinGW reprises de la livraison 1C vérifiée.\n"
          "Les licences s'appliquent à leurs composants respectifs, pas au code original du projet.\n\n"
          + f"{len(packages)} paquets tiers\n" + "\n".join(index) + "\n")
    return len(packages)


NOTICE = """PRIMAL VOXEL 2 — WINDOWS x64 — LAN

Décompressez tout le dossier puis ouvrez PrimalVoxel.exe : menu Solo / Héberger / Rejoindre.
L'hébergement depuis le menu lance automatiquement primal-server.exe à côté du client.
Le serveur conserve le monde; les invités n'ont pas besoin de copier ses fichiers.

Raccourcis fournis : Jouer.cmd (menu), Solo.cmd, Heberger.cmd, Rejoindre.cmd,
Serveur.cmd. Exemple pour un autre PC : Rejoindre.cmd 192.168.1.20:47620
Heberger.cmd démarre le serveur LAN et le client local, puis demande l'arrêt du
serveur quand le client ferme. Serveur.cmd garde une console dédiée; tapez stop
ou utilisez Ctrl+C pour sauvegarder et fermer proprement.

Les scripts utilisent worlds/solo.pvx et worlds/host.pvx sous ce dossier, ainsi
que profiles/player.token pour l'identité. Le menu graphique utilise son chemin
habituel sous %LOCALAPPDATA%\\PrimalVoxel. Le dossier portable doit être inscriptible.
server.json règle adresse, port UDP 47620, monde, seed, fréquence et capacité (8).
Le serveur LAN écoute 0.0.0.0; utilisez l'adresse IPv4 du PC hôte pour les invités.
Si Windows demande l'accès réseau, autorisez-le sur votre réseau privé de confiance.
Ce prototype utilise le mode netcode LAN Unsecure, sans authentification Internet.

ZQSD/WASD/flèches : déplacement; souris : regard; Espace : saut; Maj : accélérer;
F : vol avec collisions; en vol Espace monte et Ctrl gauche descend.
Clic gauche creuse, clic droit pose; molette/1–7 : matériau.
Échap libère la souris; F3 mesures; F11 plein écran.

Ne partagez pas profiles/player.token : il identifie votre joueur lors des retours.
Pour sauvegarder/transférer un monde, arrêtez d'abord le jeu et son serveur puis
copiez tout le dossier <monde>.pvx.regions. Le fichier server.redb éventuel fait
partie de ce dossier et doit accompagner les régions.

LICENCES contient les notices de toutes les dépendances Windows sélectionnées
par Cargo, dont Renet, netcode, redb et SHA-2 lorsqu'ils sont liés aux binaires.
SHA256SUMS.txt permet de vérifier les fichiers distribués, sans inclure vos mondes.
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    repo = Path(__file__).resolve().parents[1]
    parser.add_argument("--out", type=Path, default=repo.parent / "PrimalVoxel-2-Windows-x64")
    parser.add_argument("--baseline-notices", type=Path, default=repo.parent / "PrimalVoxel-1C-Windows-x64/LICENCES")
    parser.add_argument("--client", type=Path)
    parser.add_argument("--server", type=Path)
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    if not args.prepare_only and (not args.client or not args.server):
        parser.error("final packaging requires --client and --server, or use --prepare-only")
    for binary in (args.client, args.server):
        if binary and (not binary.is_file() or binary.read_bytes()[:2] != b"MZ"):
            parser.error(f"not a Windows executable: {binary}")
    args.out.mkdir(parents=True, exist_ok=True)
    count = collect_notices(repo, args.out, args.baseline_notices)
    shutil.copytree(repo / "tools/launch", args.out / "tools/launch", dirs_exist_ok=True)
    for name, script in (("Jouer", "menu"), ("Solo", "solo"), ("Heberger", "host"), ("Rejoindre", "join"), ("Serveur", "server")):
        write(args.out / f"{name}.cmd", '@echo off\n'
              f'powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0tools\\launch\\{script}.ps1" %*\n'
              'if errorlevel 1 pause\n')
    config = {"world": "worlds/host.pvx", "bind": "0.0.0.0:47620", "seed": 20261120, "tick_hz": 60, "max_players": 8}
    # Preserve local configuration and all saves when refreshing a package directory.
    if not (args.out / "server.json").exists():
        write(args.out / "server.json", json.dumps(config, indent=2) + "\n")
    write(args.out / "server.example.json", json.dumps(config, indent=2) + "\n")
    write(args.out / "LISEZMOI.txt", NOTICE)
    if args.prepare_only:
        print(f"Prepared launchers and {count} package notices at {args.out}; final binaries pending")
        return
    shutil.copy2(args.client, args.out / "PrimalVoxel.exe")
    shutil.copy2(args.server, args.out / "primal-server.exe")
    managed = [args.out / name for name in ("PrimalVoxel.exe", "primal-server.exe", "LISEZMOI.txt", "server.example.json")]
    managed += list(args.out.glob("*.cmd"))
    managed += [p for folder in ("LICENCES", "tools/launch") for p in (args.out / folder).rglob("*") if p.is_file()]
    hashes = [f"{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.relative_to(args.out).as_posix()}" for p in sorted(managed)]
    write(args.out / "SHA256SUMS.txt", "\n".join(hashes) + "\n")
    archive = args.out.parent / (args.out.name + ".zip")
    with ZipFile(archive, "w", ZIP_DEFLATED, strict_timestamps=False) as bundle:
        for file in [*sorted(managed), args.out / "SHA256SUMS.txt"]:
            bundle.write(file, file.relative_to(args.out).as_posix())
        # Always ship defaults, even if the local distribution has been played.
        bundle.writestr("server.json", (args.out / "server.example.json").read_bytes())
    with ZipFile(archive) as bundle:
        if bundle.testzip() is not None:
            raise ValueError("CRC incorrect dans le paquet portable")
        for file in managed:
            if bundle.read(file.relative_to(args.out).as_posix()) != file.read_bytes():
                raise ValueError(f"Archive différente du fichier validé {file}")
    print(f"Packaged 2 Windows executables and {count} third-party packages at {args.out}")


if __name__ == "__main__":
    main()
