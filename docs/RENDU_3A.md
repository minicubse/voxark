# Client Bevy et comparaison locale 3A

Le client Windows affiche le monde historique dans un second exécutable. Le client Macroquad reste disponible. Depuis la racine, initialiser l’environnement de compilation indiqué dans le README puis lancer `cargo run --release --locked -p primal-client-bevy`. Un clic capture la souris, Échap la libère. Les touches physiques WASD déplacent la caméra, Espace et Ctrl gauche changent son altitude, Maj accélère. Avec `--capture chemin.png`, F12 enregistre la fenêtre. Sans fichier monde explicite, le client utilise un stockage temporaire.

## Architecture

`primal-voxel` fournit les blocs, la génération, les révisions et le greedy mesher. `primal-streaming` ordonnance chargement, maillage et éviction, et `primal-storage` lit les régions. Le nouveau mode de meshing soustrait l’origine entière avant conversion des sommets en flottants. La logique conserve la caméra en double précision. Au-delà de 128 blocs de l’origine locale, tous les chunks affichés et la caméra changent de repère ensemble. Les positions des sauvegardes restent entières. Aucun changement de génération.

Chaque chunk non vide utilise un mesh Bevy, avec palette de blocs, normale par face et indices 32 bits. Six publications et six évictions explicites au maximum par image, plus le retrait éventuel du mesh remplacé par chaque publication. Rayon de chargement 5, de rendu 4, 24 tâches en vol, 1 024 chunks au maximum selon le budget résident. Les limites GPU physiques dépendent des allocations et du recyclage des buffers par Bevy et le pilote. Le temps de publication CPU mesure l’adoption du mesh et la mise en file dans l’ECS.

Bevy 0.20.1 est verrouillé dans Cargo.lock. Le compilateur est Rust 1.100 pour Windows GNU. Le lien GNU par défaut a produit un exécutable qui s’arrêtait avant main avec le code 0xc0000005. Clang/LLD de la chaîne LLVM-MinGW du projet a corrigé ce défaut. Les bibliothèques d’import LLVM-MinGW remplacent les scripts de liaison GNU de winapi 0.3. Le build release Bevy avec changement de linker a pris 6 min 23 s avec huit tâches et les sources de dépendances en cache. Un relien après une modification locale a pris 5,39 s.

## Mesures solo

Ryzen 7 7730U, Radeon intégrée, Windows x64, 16 Go installés. Bevy a identifié AMD Radeon (TM) Graphics, Vulkan, IntegratedGpu. Le client refuse les adaptateurs CPU et les noms de rendus logiciels connus. Macroquad utilise OpenGL. Seed 20261120, 1280 × 720, greedy scalaire. Caméra (0,5 ; 18 ; 0,5), yaw 0,78, pitch −0,32, FOV vertical 65°, plans 0,05 et 112. Les deux versions utilisent le même test de visibilité par sphère, les triangles (0,1,2) et (0,2,3), le dos des faces visible, sans ombres, overlays ni brouillard dans la mesure. Les deux demandent une présentation sans attente verticale. Stabilisation au moins une seconde, 120 images de chauffe, puis 640 images comptées par passage.

| Ordre | Client et matériau | FPS moyens | P95 (ms) | Pic RAM processus (Mio) |
|---|---|---:|---:|---:|
| 1 | Macroquad simple | 678 | 2,05 | 114 |
| 2 | Bevy simple | 448 | 2,69 | 511 |
| 3 | Bevy PBR | 376 | 3,43 | 554 |
| 4 | Bevy PBR | 401 | 3,04 | 538 |
| 5 | Bevy simple | 400 | 3,37 | 503 |
| 6 | Macroquad simple | 662 | 2,26 | 126 |

Chaque passage fixe affiche 66 chunks, 11 249 quads et 22 498 triangles. Le mode simple utilise la branche sans éclairage de StandardMaterial. Le mode PBR active son éclairage et ajoute une lumière directionnelle sans ombre. La différence observée inclut donc aussi cette lumière. Sur la scène fixe, Bevy coûte davantage en mémoire et en temps par image, malgré une cadence supérieure à 60 FPS. Le faible nombre de répétitions limite la précision de cette comparaison.

Une traversée supplémentaire de 2 400 images parcourt environ 1,98 km, avec 12 824 évictions de chunks, 14 rebasages, un maximum de 254 meshes affichables et un pic RAM échantillonné de 552 Mio. Au voisinage de −1,5 milliard et +1,5 milliard de blocs, 2 400 images avec des déplacements de 1/8 et 1/16 de bloc produisent trois rebasages. Aucun de ces deux parcours n’a présenté une image entièrement vide. Ce compteur ne détecte pas un trou isolé dans un chunk. Le test Rust vérifie la taille unitaire et l’orientation des faces à grande distance. Un essai de la fenêtre Windows confirme le changement de position par clavier et d’orientation par souris.

Les mesures par passage, les percentiles, la capture et les positions observées se trouvent dans [les résultats 3A](../benchmarks/results/3a/). Pour répéter la scène, lancer Macroquad avec `--compare --frames 640 --load-radius 5 --render-radius 4 --metrics macro.json --world monde-test.pvx`, et Bevy avec `--frames 640 --report bevy.json --material simple` ou `--material pbr`. Employer un monde de test neuf dans chaque série.
