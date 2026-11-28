# Jalon 4B.2 — Navigation locale de la wyverne

Proposition depuis `8cc2ee0ccee548481cab19b90061e29c9d1cc739`. Le nouveau mode se lance avec `--creature-behavior navigation` ou avec `"creature_behavior": "navigation"` dans la configuration du serveur. Le défaut reste `demo`. L’option `reactive` conserve la marche directe 4B.1.

## Voir le contournement

La séquence montre le jeu rendu par le client Bevy B. A se tient derrière le mur. La wyverne longe le mur, contourne son extrémité, s’arrête à 3,50 m de A puis retrouve exactement son point initial lorsque A s’éloigne. La caméra de B reste fixe. L’animation compte 29 captures, avec une durée d’affichage calculée à partir des ticks serveur. [Vue animée](../benchmarks/results/4b2/acceptance/detour.webp) · [Passage latéral](../benchmarks/results/4b2/acceptance/passage.png) · [Vue A](../benchmarks/results/4b2/acceptance/bevy-a.png) · [Vue B](../benchmarks/results/4b2/acceptance/bevy-b.png)

Pour essayer la scène depuis le dépôt, utiliser un nom de répertoire neuf. L’exemple crée une sauvegarde de forêt moderne de graine 20261120. Les huit blocs du mur sont de vrais blocs de terrain enregistrés, à X = −1, Z = 6 à 9, Y = 25 et 26.

```powershell
cargo build --locked --release -p primal-server --bin primal-server --example navigation_wall
cargo build --locked --release -p primal-client-bevy
target/release/examples/navigation_wall.exe ./essai-mur-4b2
target/release/primal-server.exe --world ./essai-mur-4b2/forest.pvx --bind 127.0.0.1:5000 --creatures 1 --creature-behavior navigation
```

Dans un autre terminal, lancer le client. Pour reproduire la position de A, se placer de l’autre côté du mur, vers (5,17 ; 26 ; 7,77), puis s’éloigner à plus de 16 m.

```powershell
target/release/primal-client-bevy.exe --connect 127.0.0.1:5000 --asset-root assets --creature-manifest creatures/wyvern_art.json --resolution 1280x720
```

## Chemin et coût borné

Seul le premier emplacement de wyverne alloue le navigateur, uniquement dans le nouveau mode. La décision `Approach` ou `Return` lui fournit la position du joueur autoritaire ou celle du point initial. La simulation garde la sélection de cible, la vitesse horizontale de 2 m/s, les rotations progressives et les transitions de locomotion 4B.1. Le serveur transmet le résultat dans les snapshots v2 existants.

- A* travaille sur 32 × 32 cases d’un mètre, à quatre voisins, avec distance de Manhattan et ordre de départage stable. Le centre de la wyverne reste dans [−15,5 ; 15,5] en X et Z. La frontière, les coûts et les parents sont conservés d’un tick au suivant.
- Le serveur traite au plus six entrées de la file par tick et 192 par recherche. Le déplacement attend un chemin complet. La vérification de praticabilité des centres est mémorisée pendant une recherche. Chaque arête est aussi testée à son milieu.
- La position des pieds suit le `GroundPatch` 4B.1. À chacun des quatre coins d’un carré de 0,70 m de côté, le serveur exige un support solide résident au niveau de la surface de référence et de l’air jusqu’à 1,80 m au-dessus des pieds. Il accepte un écart vertical maximal d’un mètre avec la surface locale. Le pas réellement exécuté est contrôlé à nouveau. Les ailes restent en dehors de cette empreinte.
- Un changement de case cible, de décision ou une édition validée dans le patch annule le trajet. Les nouvelles recherches sont espacées d’au moins 0,5 s. Après épuisement du budget ou de la file, le serveur attend une seconde avant de retenter un objectif inchangé. Une étape devenue impraticable bloque immédiatement le mouvement.
- La distance d’arrêt s’applique à l’approche dégagée du joueur. Près de lui, le serveur échantillonne le dernier segment avant de quitter les centres de cases. Au retour, il conserve les points de passage jusqu’à la case d’origine. Cette distinction a corrigé un blocage reproductible au coin du mur.

**Limites.** La recherche vise la case du joueur projetée au sol. Une case cible bloquée, hors du patch ou un détour trop long peuvent laisser la wyverne immobile même si un autre itinéraire existe à plus grande échelle. Les edits servent à ouvrir ou bloquer les passages sur la surface existante. Les ponts surélevés, sauts, chutes, tunnels à plusieurs étages et vols ne sont pas simulés. La détection reste fondée sur la distance, y compris derrière un mur. Un redémarrage reconstruit le groupe initial.

## Validation fonctionnelle

Les tests Rust utilisent la forêt réellement générée et chargée dans `World`, les mêmes lectures de voxels et la même empreinte que le serveur. Ils couvrent le détour devant un mur et une tranchée, le retour après perte de cible, l’immobilité devant une barrière fermée, sa réouverture, ainsi que l’ajout et la suppression de blocs pendant la marche. Les assertions portent sur le passage réellement emprunté, le support, la limite de vitesse, l’arrêt et le retour. Le test de suppression ouvre une brèche alors que l’ancien détour reste praticable.

Le test réseau utilise un serveur local à 60 Hz, deux clients Bevy v2 avec le modèle artistique, et le client Macroquad v1 du paquet 3D. A est amené derrière le mur par une commande serveur dont le client confirme la position. Le relevé des trois processus contient 156 ticks communs. Les ID, positions, orientations, locomotions et ticks de début de clip coïncident exactement. Les décisions sont consignées côté serveur. Les clients reçoivent la trajectoire et la locomotion.

| Tick échantillonné | Décision | Position serveur (X, Y, Z), en mètres |
|---:|---|---|
| 150 | Repos | −2,829 ; 25 ; 7,774 |
| 159 | Observation de A | −2,829 ; 25 ; 7,774 |
| 255 | Approche | −2,701 ; 25 ; 7,668 |
| 513 | Arrêt | 2,492 ; 25 ; 5,522 |
| 570 | Retour | 2,500 ; 25 ; 5,500 |
| 849 | Repos | −2,829 ; 25 ; 7,774 |

Dans les 10 snapshots d’approche et les 11 snapshots de retour où l’empreinte chevauche la bande X du mur, le centre passe à Z = 5,50 m. Le mur commence à Z = 6,00 m, soit 0,15 m de marge au-delà de la demi-empreinte.

Sur 445 échantillons de rendu alignés, l’écart maximal de position entre les deux Bevy est inférieur à 0,000001 m. B a quitté et retrouvé la zone, puis s’est reconnecté avec la même identité et une seule racine de rendu. Macroquad a supprimé un tronc puis Bevy a observé la même révision. L’édition pendant la marche est testée séparément dans la simulation serveur. [Relevé de la session](../benchmarks/results/4b2/acceptance/acceptance.json) · [Trajectoires au même tick](../benchmarks/results/4b2/acceptance/same-tick.jsonl)

### Compatibilité 4B.1

Les 137 tests existants et les quatre nouveaux tests de navigation passent sur PC 1 (141 réussis, zéro échec). Format, compilation et Clippy avec avertissements bloquants passent aussi. Les deux microbenchmarks optionnels sont ignorés dans la suite normale. Celui de navigation a également été exécuté en release.

Le test graphique relancé sur le serveur 4B.2 en mode `reactive` a comparé 131 ticks communs. Il retrouve la marche directe, l’arrêt à 3,5 m, le retour, la reconnexion et l’édition v1. Un autre lancement, sans option de comportement, a reproduit les six animations de `demo` sur 615 ticks communs. Les 471 ticks également présents dans la trace historique 4A sont identiques pour tous les champs autoritaires. Le diff depuis la base conserve les clients, les formats réseau et les assets. [Contrôles de régression](../benchmarks/results/4b2/regression.json)

## Mesures sur PC 1

Ryzen 7 7730U détecté, Radeon intégrée sélectionnée par wgpu, Vulkan AMD 23.19. La caméra (19,43,25), le rendu forêt Low en 1280 × 720, le rayon 4, la graine, le binaire Bevy et le GLB artistique sont identiques. Le harnais a alterné le serveur 4B.1 conservé à la base approuvée et le serveur 4B.2. Trois passages par cas avec ordre inversé au deuxième. Aucun benchmark ne tournait en parallèle avec une compilation Rust ou une génération d’assets. Les chiffres de cette série comparent des mondes neufs comme en 4B.1. Le mur édité fait l’objet de l’essai fonctionnel et des mesures CPU ci-dessous.

Une fenêtre Bevy a été mesurée sur 1 400 images par passage. Dans le scénario mixte, le client instrumenté a rendu 800 images par passage avec un deuxième Bevy et Macroquad v1 actifs sur le même PC. Les chiffres sont les médianes par scénario. Le P99 serveur est celui de la pire fenêtre de 120 ticks de chaque passage, avant calcul de la médiane.

| Clients / wyvernes | Serveur | FPS moyens | 1 % low | P99 image ms | P99 serveur ms | Hz serveur | Pas IA écoulé µs | RAM Bevy / serveur Mio |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| 1 / 1 | 4B.1 reactive | 179,8 | 92,0 | 10,18 | 1,76 | 60,00 | 3,14 | 832,4 / 9,1 |
| 1 / 1 | 4B.2 navigation | 185,1 | 95,7 | 7,23 | 1,42 | 60,00 | 3,22 | 896,7 / 9,0 |
| 1 / 4 | 4B.1 reactive | 156,9 | 94,4 | 9,72 | 1,59 | 60,01 | 2,77 | 831,8 / 9,2 |
| 1 / 4 | 4B.2 navigation | 157,1 | 92,0 | 10,11 | 1,60 | 60,00 | 3,10 | 834,7 / 9,1 |
| 3 / 1 | 4B.1 reactive | 16,6 | 7,5 | 120,30 | 2,55 | 60,00 | 3,27 | 903,5 / 10,9 |
| 3 / 1 | 4B.2 navigation | 135,7 | 69,6 | 13,06 | 2,52 | 60,00 | 3,68 | 902,2 / 11,0 |

**Session mixte reactive.** Les trois moyennes individuelles sont 135,7, 16,5, 16,6 FPS. Sur 2 400 images, 1568 dépassent 16,67 ms, dont 1351 dépassent 33,33 ms. Maximum 177,30 ms. Un passage de contrôle supplémentaire, conservé séparément, donne 129,9 FPS et 73,1 en 1 % low.

**Session mixte navigation.** Les trois moyennes individuelles sont 138,5, 135,7, 17,4 FPS. Sur 2 400 images, 784 dépassent 16,67 ms, dont 669 dépassent 33,33 ms. Maximum 151,75 ms. Un passage de contrôle supplémentaire, conservé séparément, donne 135,0 FPS et 67,4 en 1 % low.

La cible de 60 FPS a été dépassée dans les deux modes lors du passage de contrôle. Lors de la série initiale, un passage navigation et deux passages de référence sont descendus sous 18 FPS, tandis que le serveur continuait à 60 Hz. Les rapports contiennent chaque frametime et les fenêtres serveur. La cause de cette variation sur PC 1 n’a pas été isolée. Les différences de FPS entre les deux exécutables ne permettent pas d’attribuer un gain au planificateur. [Passages à un client](../benchmarks/results/4b2/one-client) · [Passages mixtes](../benchmarks/results/4b2/three-clients) · [Contrôle mixte supplémentaire](../benchmarks/results/4b2/three-clients-repeat)

### Budget du pas de simulation

Sur la forêt résidente, le même joueur alterne 720 ticks proches et 720 ticks éloignés. Chaque mode reçoit les mêmes entrées et obstacles. Le témoin utilise la logique directe 4B.1 dans le même binaire de test. Le CPU du thread est cumulé sur 2 400 000 ticks par cas. Une deuxième boucle de 240 000 ticks relève le temps écoulé de chaque pas et le nombre maximal de sondes du terrain. Ce coût inclut le choix de cible, le déplacement, le calcul de trajet et les contrôles d’appui. Le repère pour le pas complet est 1 ms sur les 16,67 ms disponibles à 60 Hz.

| Terrain | Mode | CPU moyen µs/tick | P99 écoulé µs | P99,9 µs | Max µs | Ticks > 1 ms | Sondes max/tick |
|---|---|---:|---:|---:|---:|---:|---:|
| Mur | reactive | 0,163 | 0,5 | 0,5 | 12,8 | 0 | 1 |
| Mur | navigation | 0,247 | 0,6 | 9,2 | 18,7 | 0 | 30 |
| Trou | reactive | 0,156 | 0,5 | 0,5 | 41,4 | 0 | 1 |
| Trou | navigation | 0,241 | 0,6 | 9,2 | 29,5 | 0 | 30 |
| Barrière fermée | reactive | 0,169 | 0,5 | 0,5 | 44,3 | 0 | 1 |
| Barrière fermée | navigation | 0,892 | 7,4 | 9,3 | 44,5 | 0 | 26 |

Le cas fermé oblige A* à épuiser sa recherche puis à attendre avant de recommencer. Le plafond algorithmique est de 65 sondes par tick pour une wyverne. Le temps maximal observé dépend de la machine et de son ordonnanceur. [Relevé CPU](../benchmarks/results/4b2/cpu.json)

## Reproduire les contrôles

Exécuter les quatre contrôles Rust depuis la racine. Le microbenchmark CPU s’exécute séparément en release. Pour la session graphique sous Windows, installer Pillow et psutil et adapter le chemin du binaire v1. Les répertoires de résultats doivent être neufs.

```powershell
cargo fmt --check
cargo check --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo test --locked --release -p primal-server --bin primal-server navigation_cpu -- --ignored --nocapture

$server = "target/release/primal-server.exe"
$bevy = "target/release/primal-client-bevy.exe"
$macro = "../PrimalVoxel-3D-Windows-x64/PrimalVoxel.exe"
python tools/test-reactive-creatures.py --server $server --bevy $bevy --macro $macro --navigation-wall target/release/examples/navigation_wall.exe --output benchmarks/tmp/nav-acceptance
```

Pour la comparaison, construire le serveur de référence dans une cible distincte. Les deux modes utilisent le même exécutable Bevy et les mêmes assets.

```powershell
git worktree add --detach ../voxark-reference-4b1 8cc2ee0ccee548481cab19b90061e29c9d1cc739
cargo build --release --locked --manifest-path ../voxark-reference-4b1/Cargo.toml --target-dir target/reference-4b1 -p primal-server
$reference = "target/reference-4b1/release/primal-server.exe"
python tools/benchmark-network-creatures.py --server $server --reactive-server $reference --client $bevy --assets assets --creature-manifest creatures/wyvern_art.json --behaviors reactive navigation --counts 1 4 --frames 1400 --repeats 3 --output benchmarks/tmp/nav-one
python tools/benchmark-network-creatures.py --server $server --reactive-server $reference --client $bevy --assets assets --creature-manifest creatures/wyvern_art.json --behaviors reactive navigation --counts 1 --frames 800 --repeats 3 --mixed-macroquad $macro --output benchmarks/tmp/nav-mixed
```
