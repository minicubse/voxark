# Jalon 4A — Wyverne serveur

## Plan de livraison établi depuis main 4344026

1. Simuler à cadence fixe un groupe borné de wyvernes dans le noyau Rust. Reconstruire la démonstration au démarrage et mesurer les frontières des six états.
2. Étendre le protocole v1 par négociation. Les champs et discriminants des messages existants restent compatibles. Chaque snapshot de créatures décrit l’ensemble visible au tick donné. La perte d’un paquet est réparée par le suivant.
3. Associer chaque identifiant réseau à une instance Bevy. Interpoler dans le temps serveur et convertir en origine locale au moment de dessiner. Vérifier arrivées tardives, pertes, sortie de zone et reconnexions.
4. Tester le serveur avec deux clients Bevy et Macroquad. Mesurer séparément 0, 1, 4 et 12 créatures sur le matériel détecté, puis vérifier les tests historiques, la compilation et Clippy.

La commande artistique `wyvern` reste indépendante sur PC 2. Les six clips existants couvrent le besoin du jalon.

## Jouer à deux

Depuis la racine du dépôt, compiler avec `cargo build --release --locked -p primal-server -p primal-client-bevy`. Exécuter les trois commandes dans trois terminaux PowerShell. Adapter le chemin des exécutables si `CARGO_TARGET_DIR` est défini.

```powershell
.\target\release\primal-server.exe --bind 127.0.0.1:47620 --world demo-4a.pvx --preset forest-modern --creatures 1
.\target\release\primal-client-bevy.exe --connect 127.0.0.1:47620 --asset-root assets --identity bevy-a.identity
.\target\release\primal-client-bevy.exe --connect 127.0.0.1:47620 --asset-root assets --identity bevy-b.identity
```

La console serveur accepte `creatures`, `spawn-creature` et `despawn-creature <id>`. Le serveur démarre avec une wyverne par défaut et accepte de zéro à douze créatures. Les palettes alternent entre Jade, Braise et Obsidienne. Les deux clients partagent le cycle du serveur. Pour l’inspection locale, lancer Bevy hors connexion avec `--creatures 1`.

## Autorité, compatibilité et persistance

Le noyau Rust associe à chaque créature un identifiant distinct de ceux des joueurs, une espèce, une palette, une position monde, un lacet et le tick d’entrée dans son état. À 60 Hz, la première wyverne entre successivement en repos, marche, décollage, vol battu, plané et atterrissage aux ticks 0, 300, 487, 550, 826 et 1138. Le cycle recommence à 1204. La trajectoire utilise le relief généré pour les appuis. Les éditions de terrain postérieures peuvent modifier ces appuis. Le mouvement monde vient du serveur, le squelette joue ses clips sur place.

Une reconnexion retrouve la simulation en cours. Un redémarrage reconstruit le groupe initial configuré et recommence le cycle au tick zéro. Les créations et suppressions administratives sont temporaires. Le test de redémarrage rouvre le même monde et reconnecte la même identité joueur.

Le serveur accepte les clients v1 et v2. Les anciens clients Macroquad et Bevy continuent de recevoir le terrain et les joueurs. Le nouveau client Bevy demande la v2 et signale une incompatibilité si le serveur comprend uniquement la v1. Toutes les trois mises à jour, un client v2 reçoit le tick, la fréquence et la liste complète des créatures présentes dans son rayon horizontal de trois chunks. Une liste vide signale la sortie de zone. La liste suivante répare une perte. Aucun os ne transite sur le réseau.

Bevy conserve jusqu’à seize snapshots par créature, ignore les ticks obsolètes ou dupliqués et affiche la scène avec 100 ms de retard. Il interpole la position monde et le lacet par le chemin angulaire court. Un trou supérieur à 400 ms interrompt l’interpolation entre les deux extrémités. Le client borne l’affichage au dernier état reçu. L’origine flottante s’applique au rendu. Chaque retour en zone reconstruit la pose et la phase à partir du tick serveur. Les clips cycliques bouclent, le décollage et l’atterrissage se bornent à leur durée. Le fondu dure 220 ms. Le manifeste v1 sélectionne le modèle indépendamment de l’identité réseau.

## Acceptation sur PC 1

Un serveur, deux processus Bevy Vulkan et le Macroquad v1 du ZIP 3D ont tourné ensemble. Les deux Bevy ont reçu des états strictement identiques au serveur sur **536 ticks communs**, couvrant les six clips. Exemple au tick 360, les trois journaux donnent l’identifiant `9223372036854775809`, la palette Jade, l’état Walk commencé au tick 300 et la position monde `[-3.3507516792, 25, 7.5122543811]`.

Pour comparer les images calculées à des instants différents, le harnais aligne deux échantillons de rendu sur le même tick dans un intervalle de snapshots. Sur 439 comparaisons, les écarts maximaux entre les clients atteignent 0,00000336 m, 0,000000221 rad et 0,000000170 s de phase. Comparée directement aux durées du GLB et au tick d’entrée serveur, la phase de chacun des deux clients s’écarte de moins de 0,000000113 s. Les 766 positions disposant des deux bornes serveur dans le journal s’écartent de l’interpolation autoritaire de moins de 0,00000179 m.

Le second client a rejoint le cycle en cours, quitté puis retrouvé la zone et s’est reconnecté. Aucun doublon de racine. Macroquad a retiré le bloc en `[-17, 28, 2]` et Bevy a reçu puis remeshé la révision 1. Les pertes et inversions de snapshots sont couvertes par les tests Rust. Les processus graphiques ont communiqué en boucle locale sur PC 1. [Rapport et empreintes des exécutables](../benchmarks/results/4a/acceptance/acceptance.json) · [Capture Bevy A](../benchmarks/results/4a/acceptance/bevy-a.png) · [Capture Bevy B](../benchmarks/results/4a/acceptance/bevy-b.png)

## Performances mesurées

PC 1 utilise un Ryzen 7 7730U, un GPU intégré AMD Radeon (TM) Graphics, 15,38 Gio de RAM détectée et Windows 11 Famille 10.0.26200. Pilote Windows 31.0.21925.1001, Vulkan AMD 23.19, mode d’alimentation Utilisation normale. Serveur et client release en boucle locale, seed 20261120, forêt Low, 1280 × 720, rayon graphique 4, caméra fixe `19,43,25,-2.52,-0.40`. Chaque passage ouvre un monde neuf, attend le chargement puis mesure 4200 images. Trois passages par scénario, ordre inversé au deuxième, sans compilation concurrente. Chaque valeur du tableau est la médiane de ses trois passages. Le témoin 3D utilise les deux exécutables historiques en réseau au même cadrage.

| Scénario | FPS moyens | 1 % low | P99 (ms) | Pic client (Mio) | CPU client (cœurs) | Animation CPU (µs/image) |
|---|---:|---:|---:|---:|---:|---:|
| Témoin 3D v1, 0 | 186,6 | 115,6 | 7,50 | 600,1 | 2,41 | 243 |
| 4A, 0 | 194,8 | 121,0 | 7,10 | 609,0 | 2,43 | 271 |
| 4A, 1 | 185,9 | 108,3 | 8,25 | 611,0 | 2,51 | 355 |
| 4A, 4 | 188,8 | 113,2 | 7,90 | 611,0 | 2,65 | 376 |
| 4A, 12 | 177,0 | 107,6 | 8,26 | 601,2 | 2,73 | 416 |

| Wyvernes 4A | Simulation créatures (ms/tick) | Tick serveur P50 (ms) | P99 pire fenêtre (ms) | RSS serveur en fin de passage (Mio) | CPU serveur (cœurs) | Flux créatures (octets/s/client) |
|---:|---:|---:|---:|---:|---:|---:|
| 0 | 0,00138 | 0,912 | 1,81 | 9,04 | 0,059 | 100 |
| 1 | 0,00239 | 0,848 | 1,89 | 9,02 | 0,060 | 959 |
| 4 | 0,00409 | 0,847 | 1,96 | 9,16 | 0,059 | 3 537 |
| 12 | 0,00830 | 0,927 | 2,11 | 9,17 | 0,063 | 10 421 |

La cadence mesurée atteint 60,00 ticks/s et 20,00 snapshots de créatures/s par client dans les quatre scénarios 4A. Le flux compte la charge utile applicative et inclut les listes vides. L’animation mesure la durée CPU de l’étape Bevy. Le skinning GPU n’est pas chronométré isolément. Les douze instances figurent dans la vue principale sur les trois passages à douze. Face au témoin 4A vide, douze créatures coûtent environ 9 % en FPS moyens et 11 % en 1 % low. La cible de 60 FPS moyens est atteinte dans cette scène. Les 136,5 FPS à douze du jalon 3D correspondent à un autre scénario local. Le serveur bi-Xeon et le WAN restent à mesurer. [Médianes](../benchmarks/results/4a/network/summary.json) · [Relevés par passage](../benchmarks/results/4a/network)

## Reproduire les contrôles

Utiliser Python 3.11 ou supérieur avec `psutil` et `Pillow`, une session Windows graphique et les binaires release. Depuis la racine, adapter les deux répertoires de binaires et choisir des sorties neuves.

```powershell
$bin4a = "target/release"
$bin3d = "../PrimalVoxel-3D-Windows-x64"
python tools/test-creature-network.py $bin4a benchmarks/tmp/acceptation-4a --legacy-macroquad "$bin3d/PrimalVoxel.exe" --asset-root assets
python tools/benchmark-network-creatures.py --client "$bin4a/primal-client-bevy.exe" --server "$bin4a/primal-server.exe" --old-client "$bin3d/PrimalVoxel-Bevy.exe" --old-server "$bin3d/primal-server.exe" --assets assets --frames 4200 --repeats 3 --output benchmarks/tmp/mesures-4a
```

Les quatre contrôles Rust passent sur ce jalon.

```sh
cargo fmt --check
cargo check --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

Les 132 tests couvrent notamment les frontières du cycle, le protocole, l’arrivée tardive, la reconnexion, le redémarrage, les grandes coordonnées et la réconciliation des snapshots. La prochaine étape est la revue de la branche moteur. Quand PC 2 livrera son manifeste, le valider avec le validateur 3D puis rejouer l’acceptation avec l’asset candidat avant de l’intégrer.
