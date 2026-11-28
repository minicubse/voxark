# Jalon 4B.1 — Première wyverne réactive

Le jalon a été intégré à `main` par la [PR #5](https://github.com/minicubse/voxark/pull/5) au commit `061bc22e5b96544aa631219636ca1299068e4d77`. Le serveur choisit la cible, la direction et l’allure. Deux clients Bevy ont suivi la même wyverne artistique pendant qu’un client Macroquad v1 participait à la session.

## Essayer le comportement

Depuis la racine du dépôt, avec les assets Git LFS présents et un répertoire de monde neuf disponible, lancer dans deux terminaux :

```powershell
cargo run --release -p primal-server -- --bind 127.0.0.1:5000 --world ./benchmarks/tmp/4b1-manual.pvx --preset forest-modern --creatures 1 --creature-behavior reactive
cargo run --release -p primal-client-bevy -- --connect 127.0.0.1:5000 --asset-root assets --creature-manifest creatures/wyvern_art.json --resolution 1280x720
```

Approcher à moins de 12 m, attendre l’observation puis reculer au-delà de 16 m de la créature. La commande console serveur `creatures` donne le dernier snapshot publié avec sa décision et l’identifiant de sa cible. Un fichier de configuration accepte `"creature_behavior": "reactive"`. Le défaut `demo` conserve les trajectoires et les six clips 4A. La sélection du manifeste artistique s’effectue par client.

## Simulation autoritaire

En 4A les trajectoires sont des fonctions du tick, échantillonnées avant la simulation des joueurs. En 4B.1, chaque tick intègre d’abord les entrées des joueurs et résout les corrections en attente. La première wyverne lit ensuite les positions des pieds des joueurs prêts. À 60 Hz, le serveur publie un snapshot tous les trois ticks. La console conserve la décision associée exactement à ce snapshot.

| Décision | Déclenchement et mouvement |
|---|---|
| Rest | Immobile au point initial. Acquiert le joueur le plus proche à 12 m inclus en 3D. En cas d’égalité, choisit le plus petit identifiant. |
| Observe | Garde sa position et tourne vers la cible pendant au moins 1,5 s. La rotation est limitée à 180°/s. |
| Approach | Suit la cible conservée, à 2 m/s au maximum sur le plan horizontal. Commence à marcher quand l’écart d’orientation descend à 45° ou moins. |
| Stop | S’immobilise à 3,5 m horizontalement. Reprend à partir de 4,5 m après au moins 1,5 s dans cet état. |
| Return | Une cible déconnectée ou à plus de 16 m en 3D déclenche le retour au point initial. Peut acquérir une nouvelle cible après 1,5 s de retour. |

La mémoire de décision appartient au premier emplacement de la population. Une réapparition y reçoit un nouvel identifiant. Les autres wyvernes suivent la trajectoire 4A. La recherche parcourt au plus une fois les joueurs prêts (32 maximum configurables), sans allocation par cible. Le contrôle local du terrain examine quatre colonnes par pas. La mémoire de l’IA est de taille fixe. Le redémarrage reconstruit le groupe initial.

Le serveur émet `IdleGround` ou `Walk` selon le mouvement réalisé. Il conserve `state_start_tick` tant que le clip continue. L’identité, le format des messages v1/v2 et l’interpolation des clients utilisent le code du jalon 4A.

### Terrain et limites

La détection utilise la distance entre la wyverne et le joueur. Elle fonctionne aussi derrière un mur, car le serveur ne teste pas la ligne de vue. Le contrôle des voxels intervient au moment de marcher.

Le centre de la créature reste entre −15,5 et +15,5 m en X et Z. L’altitude suit la surface prééchantillonnée du `GroundPatch`. Avant chaque déplacement, le serveur vérifie le support réel et l’espace libre dans les voxels résidents, sur quatre points distants de 0,35 m du centre en X et Z. Une surface inconnue, un trou, un mur ou une limite de patch bloque le pas et donne `IdleGround`. La créature peut reprendre si le pas redevient possible. Un obstacle persistant peut aussi empêcher le retour. Cette approche ne cherche aucun détour. Les ailes ne participent pas à la collision et un sol supprimé sous une créature stationnaire ne déclenche pas de chute.

## Preuves multijoueurs

Monde neuf, graine 20261120, session locale avec serveur à 60 Hz, deux fenêtres Bevy v2 et le binaire Macroquad v1 du paquet 3D. Le harnais place les joueurs via le serveur puis vérifie les positions reçues par les clients. Il oriente les caméras indépendamment pour les captures. La position du joueur A est passée à (5,171 ; 26 ; 7,774) à 8 m horizontalement de la wyverne. B était déjà connecté à la simulation en cours et observait depuis l’extérieur du rayon d’acquisition.

| Tick observé | Décision | Ce que voit le test |
|---:|---|---|
| 123 | Rest | La wyverne attend en (−2,829 ; 25 ; 7,774). |
| 132 | Observe | Le serveur choisit A, joueur 1. |
| 234 | Approach | Les deux clients jouent Walk. |
| 363 | Stop | La wyverne atteint (1,671 ; 25 ; 7,774), à 3,50 m de A. |
| 444 | Return | A s’est éloigné. Les deux clients voient le retour en marche. |
| 579 | Rest | Retour au point initial, au repos. |

Sur 131 ticks communs couvrant les cinq décisions, les deux clients rapportent exactement l’ID, la position, le yaw, la locomotion et le début du clip envoyés par le serveur. Les 267 échantillons de rendu alignés donnent des écarts maximaux de 0,00000054 m, 0,00000013 rad et 0,00000018 s de phase. B a quitté la zone puis l’a réintégrée. Sa reconnexion a restauré son identité et une seule racine de rendu. Macroquad a enlevé le tronc en (−17 ; 28 ; 2), de la révision 0 à 1, et Bevy a également remaillé le bloc. Les captures prises à l’arrêt montrent la tête face au joueur A et la même créature de côté depuis B. [Télémétrie](../benchmarks/results/4b1/reactive/acceptance.json) · [Snapshots des trois processus](../benchmarks/results/4b1/reactive/same-tick.jsonl) · [Vue A](../benchmarks/results/4b1/reactive/bevy-a.png) · [Vue B](../benchmarks/results/4b1/reactive/bevy-b.png)

En mode démo par défaut, un second test complet a observé les six animations, l’arrivée tardive, la sortie et le retour en zone, la reconnexion et l’édition de terrain depuis v1. Il a comparé 656 ticks communs et 699 échantillons rendus. Les 501 ticks qui recoupent l’enregistrement d’acceptation 4A donnent exactement les mêmes champs autoritaires. [Contrôle démo](../benchmarks/results/4b1/demo-control.json) · [Trace actuelle](../benchmarks/results/4b1/demo-authority.jsonl) · [Trace 4A](../benchmarks/results/4a/acceptance/same-tick.jsonl)

## Performances mesurées sur PC 1

Le système a identifié un Ryzen 7 7730U. wgpu a sélectionné AMD Radeon (TM) Graphics, `IntegratedGpu`, backend Vulkan. Monde local neuf, serveur release à 60 Hz, même binaire Bevy, même GLB artistique à 23 joints, forêt Low, 1280 × 720, rayon 4, caméra fixe `19,43,25,-2.52,-0.40`, graine 20261120. Trois passages par cas, ordre inversé au deuxième passage. Les compilations et les tests fonctionnels étaient terminés avant les mesures.

Le tableau donne les médianes des trois passages. Le premier scénario rend une fenêtre Bevy. Le scénario d’acceptation en rend trois sur le même PC (deux Bevy et un Macroquad v1). Les FPS et la mémoire client concernent le Bevy instrumenté. Les fenêtres d’acceptation fonctionnelle et les captures, en 960 × 540, constituent un essai distinct.

| Clients / wyvernes | Mode | FPS moyens | 1 % low | P99 image ms | Tick P50 / P99¹ ms | IA² µs/tick | RAM Bevy / serveur³ Mio |
|---|---|---:|---:|---:|---:|---:|---:|
| 1 / 1 | demo | 207,6 | 126,0 | 7,23 | 0,81 / 1,38 | 0,00 | 833,2 / 9,2 |
| 1 / 1 | reactive | 196,9 | 109,8 | 8,64 | 1,02 / 1,70 | 2,86 | 832,3 / 9,1 |
| 1 / 4 | demo | 183,4 | 112,5 | 7,68 | 0,87 / 1,47 | 0,00 | 833,1 / 9,0 |
| 1 / 4 | reactive | 175,5 | 112,3 | 8,41 | 1,09 / 1,78 | 2,87 | 832,3 / 9,3 |
| 3 / 1 | demo | 163,0 | 87,6 | 9,75 | 1,07 / 2,33 | 0,00 | 899,4 / 10,7 |
| 3 / 1 | reactive | 159,1 | 86,6 | 10,18 | 1,18 / 2,23 | 3,53 | 899,5 / 10,8 |

¹ Tick de travail serveur, P99 de la pire fenêtre complète de 120 ticks par passage. La cadence médiane observée reste entre 60,00 et 60,01 Hz.

² Temps écoulé du pas réactif sur le thread serveur, lecture des joueurs et contrôle du terrain compris. En démo, l’IA est désactivée.

³ Pic du working set Bevy et working set serveur en fin de mesure. La mémoire GPU dédiée au rendu n’a pas été isolée.

**Seuil de 60 FPS.** En session à trois clients, le Bevy mesuré atteint 159,1 FPS en réactif contre 163,0 en démo. Une image réactive sur 2 400 dépasse 16,67 ms (24,61 ms). Aucune ne dépasse 33,33 ms. Aucune image démo sur 2 400 ne dépasse 16,67 ms. Ces fenêtres courtes couvrent le rendu stabilisé. Elles n’établissent pas une borne de latence lors d’une connexion, de la génération initiale ou d’une longue partie. Le comportement modifie aussi la pose et la position du modèle, donc l’écart de FPS ne mesure pas à lui seul le coût de décision.

**CPU de l’IA.** Un microbenchmark release mesure le temps CPU du thread avec `ThreadTime` sur quatre millions de ticks par mode. Il alterne un joueur proche puis éloigné tous les 480 ticks avec terrain réel résident et retour à l’origine. Démo, 31,25 ms cumulées (0,008 µs/tick de boucle). Réactif, 656,25 ms (0,164 µs/tick). Ce coût en boucle chaude complète le temps écoulé mesuré en session. Les passages bruts conservent aussi le CPU du processus serveur et les frametimes individuels. [Mesures à un client](../benchmarks/results/4b1/one-client) · [Mesures à trois clients](../benchmarks/results/4b1/three-clients) · [Temps CPU du thread](../benchmarks/results/4b1/thread-cpu.json)

## Validation et reproduction

Les contrôles `cargo fmt --check`, `cargo check --locked --workspace`, `cargo clippy --locked --workspace --all-targets -- -D warnings` et `cargo test --locked --workspace` passent sur PC 1. La suite compte 137 tests réussis et zéro échec. Le microbenchmark CPU, ignoré dans cette suite, a été exécuté séparément en release. Les tests couvrent notamment les seuils, l’ordre des joueurs à égalité, la perte de cible, le retour, les éditions de voxels, les limites du patch et un rejeu déterministe de 2 500 ticks.

Avec Python, Pillow et psutil, adapter les trois exécutables à son installation. Tous les résultats s’écrivent dans de nouveaux répertoires.

```powershell
$server = "target/release/primal-server.exe"
$bevy = "target/release/primal-client-bevy.exe"
$macro = "../PrimalVoxel-3D-Windows-x64/PrimalVoxel.exe"
python tools/test-reactive-creatures.py --server $server --bevy $bevy --macro $macro --output benchmarks/tmp/net-reactive
python tools/test-creature-network.py target/release benchmarks/tmp/net-demo --legacy-macroquad $macro --creature-manifest creatures/wyvern_art.json
cargo test --locked --release -p primal-server ai_thread_cpu -- --ignored --nocapture
python tools/benchmark-network-creatures.py --server $server --client $bevy --assets assets --creature-manifest creatures/wyvern_art.json --behaviors demo reactive --counts 1 4 --frames 1400 --repeats 3 --output benchmarks/tmp/perf-4b1
python tools/benchmark-network-creatures.py --server $server --client $bevy --assets assets --creature-manifest creatures/wyvern_art.json --behaviors demo reactive --counts 1 --frames 800 --repeats 3 --mixed-macroquad $macro --output benchmarks/tmp/perf-4b1-mixed
```
