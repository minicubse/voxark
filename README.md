# Primal Voxel — Vallée originelle

Version 1C pour Windows x64. À pied ou en vol, vous pouvez quitter la vallée et explorer le terrain généré au fil du trajet. Les sept matériaux restent disponibles pour creuser et construire. Les modifications sont écrites en arrière-plan.

## Jouer

Dans la distribution Windows, lancez **PrimalVoxel.exe**. Cliquez dans la fenêtre pour capturer la souris. Le premier clic entre en jeu. La sauvegarde active se trouve dans le dossier `%LOCALAPPDATA%\PrimalVoxel\world.pvx.regions`. À la première ouverture, une sauvegarde v1 est importée en gardant son fichier original. Pour transférer le monde actuel, copiez le dossier régional après fermeture. L'option `--world "chemin\autre.pvx"` sélectionne un autre monde.

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

L'option `--mesher faces|greedy|binary` sélectionne l'algorithme à comparer. En jeu, `--tick-rate 30` à `--tick-rate 120` règle la fréquence de simulation. Le parcours de mesure impose 60 Hz pour comparer les mêmes déplacements.

Le test d'intégration Windows utilise Python 3 standard. `python tools/test-window.py target/release/primal-client.exe` ouvre deux fois le client sur une sauvegarde temporaire. Il envoie les événements clavier et souris à la fenêtre du jeu et vérifie les blocs écrits après destruction puis repose.

Le client accepte quatre rayons exprimés en chunks. Par défaut, chargement 8, rendu 7, priorité de simulation 1 et anticipation 2. Le plafond effectif est de 1 024 chunks avec 64 Mio de budget de résidence, quatre workers, 24 tâches et douze maillages acceptés par frame. Les touches − et = ajustent la portée en jeu.

```powershell
primal-client.exe --scenario teleport --frames 840 --profile teleport.csv
primal-client.exe --smoke --edit-route --valley-only --frames 1320 --metrics vallee.json
cargo test --locked --release -p primal-streaming --test travel -- --nocapture
cargo run --locked --release -p primal-voxel --example compare_meshers
```

Les scénarios disponibles sont `valley`, `region512`, `region1024`, `traverse`, `teleport` et `edits`. `--help` détaille les limites de workers, mémoire, publications et temps du fil principal. Le mode `--valley-only` conserve le domaine historique pour comparer deux exécutables sur la même géométrie.

Le [statut du projet](docs/PROJECT_STATUS.md) donne les vérifications et la prochaine tâche. Les [mesures](docs/PERFORMANCE.md), les [limites](docs/KNOWN_ISSUES.md) et les [décisions moteur](docs/DECISIONS.md) sont suivies dans ce dépôt.

## Parcours de streaming 1C

Les scénarios `walk`, `sprint`, `fly`, `traverse`, `teleport`, `return`, `moving-edits` et `long-flight` utilisent des mondes temporaires. Exemple : `PrimalVoxel.exe --scenario traverse --frames 840 --preload-radius 2`. Le [rapport 1C](docs/STREAMING_1C.md) décrit la comparaison A/B et renvoie aux mesures brutes.
