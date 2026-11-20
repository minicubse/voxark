# Primal Voxel — Vallée originelle

Première tranche jouable pour Windows x64. Explorez la vallée, creusez sous les arbres et bâtissez avec sept matériaux. Chaque bloc accepté est sauvegardé après l'édition. Le terrain conserve sa forme à la prochaine ouverture.

## Jouer

Dans la distribution Windows, lancez **PrimalVoxel.exe**. Cliquez dans la fenêtre pour capturer la souris. Le premier clic entre en jeu. La sauvegarde par défaut se trouve dans `%LOCALAPPDATA%\PrimalVoxel\world.pvx`. L'option `--world "chemin\autre.pvx"` permet d'utiliser un autre fichier.

| Action | Commande |
| --- | --- |
| Marcher et regarder | ZQSD, WASD ou flèches et souris |
| Sauter, sprinter | Espace, Maj gauche |
| Activer le vol avec collisions | F. Espace monte, Ctrl gauche descend |
| Détruire, poser | Clic gauche, clic droit sur une face à portée |
| Choisir le matériau | Molette ou touches 1 à 7 |
| Libérer la souris | Échap |
| Mesures, sauvegarde manuelle, plein écran | F3, F5, F11 |
| Modifier la distance d'affichage | Touches du rang supérieur − et = |

Le terrain revient depuis le disque. Le personnage réapparaît près du centre dans un emplacement libre. Un problème de lecture ou d'écriture est affiché dans le client.

## Compiler et vérifier

Depuis la racine du dépôt, utiliser Rust 1.100.0 et Cargo.lock. Sur un Windows x64 sans compilateur, ce script prépare Rust et LLVM-MinGW dans `.tools` pour la session PowerShell courante. Il vérifie les téléchargements et ne requiert pas de compte administrateur.

```powershell
. ./tools/env-windows.ps1 -Install
cargo build --locked --release -p primal-client
cargo run --locked --release -p primal-client
```

Aux ouvertures suivantes du terminal, utiliser `. ./tools/env-windows.ps1`. Pour réutiliser le cache déjà installé par Codex sur l'ordinateur de développement, exécuter `. ./tools/env-windows.ps1 -Cache (Resolve-Path ../../work).Path` depuis ce dépôt.

```powershell
cargo fmt --check
cargo check --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo run --locked --release -p primal-voxel --example benchmark
cargo run --locked --release -p primal-storage --example benchmark_save
./tools/measure-client.ps1 -Executable ./target/release/primal-client.exe -Resolution 1280x720
```

Pour reproduire un test graphique court avec une image, lancer `primal-client.exe --smoke --frames 300 --resolution 1280x720 --capture apercu.png --metrics mesures.json`. Le parcours utilise un monde temporaire. Avec `--world` explicite, il exige un fichier neuf.

L'option `--mesher faces|greedy` sélectionne l'algorithme à comparer. En jeu, `--tick-rate 30` à `--tick-rate 120` règle la fréquence de simulation. Le parcours de mesure impose 60 Hz pour comparer les mêmes déplacements.

Le test d'intégration Windows utilise Python 3 standard. `python tools/test-window.py target/release/primal-client.exe` ouvre deux fois le client sur une sauvegarde temporaire. Il envoie les événements clavier et souris à la fenêtre du jeu et vérifie les blocs écrits après destruction puis repose.

Le [statut du projet](docs/PROJECT_STATUS.md) donne les vérifications et la prochaine tâche. Les [mesures](docs/PERFORMANCE.md), les [limites](docs/KNOWN_ISSUES.md) et les [décisions moteur](docs/DECISIONS.md) sont suivies dans ce dépôt.
