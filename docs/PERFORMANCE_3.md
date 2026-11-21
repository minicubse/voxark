# Comparaison réseau Macroquad et Bevy

## Protocole

Un serveur Rust 60 Hz, une identité, un seul joueur et la même vallée historique (seed 20261120). Windows x64, Ryzen 7 7730U, Radeon intégrée et 16 Go installés. Les deux clients demandent 1280 × 720, sans attente verticale ni plafond FPS. Macroquad utilise OpenGL. Bevy indique AMD Radeon (TM) Graphics, IntegratedGpu, Vulkan, pilote AMD 23.19. Le contrôle de l’adaptateur rejette un rendu CPU ou un nom de fallback logiciel connu. La comparaison utilise le greedy commun, la même palette, le même winding, aucun backface culling et un matériau sans éclairage dynamique. Le test simple/PBR figure dans [le rapport 3A](RENDU_3A.md).

Le serveur envoie la même zone de 7 × 7 × 8 chunks. Chaque client applique un rayon de rendu de quatre chunks, un FOV vertical de 65° et les mêmes tests de visibilité. La caméra part de (0,5, 18, 0,5). Le trajet mobile translate l’œil de 4 blocs en X et 2 en Z et tourne de 0,10 radian. Le joueur autoritaire reste immobile pour garantir la même zone d’intérêt. Le parcours en mouvement du streamer solo est mesuré séparément en 3A.

Après réception des 392 chunks, vidage de la file de meshing, stabilisation d’une seconde et 120 images de chauffe, chaque passage mesure 720 intervalles muraux. Quatre répétitions par client et par trajet, ordre inversé à chaque répétition. Les 16 passages ont abouti. Une empreinte ordonnée des nombres de chunks et quads vérifie la parité sur chacune des 720 images. Les deux clients terminent à 54 chunks et 10 300 quads en fixe, puis 53 chunks et 10 154 quads en mouvement. Deux triangles par quad. Aucune image mesurée entièrement vide.

## Résultats

Médiane des quatre passages. La dernière colonne donne le plus haut pic du working set Windows sur les quatre exécutions, démarrage inclus. Le 1 % low est l’inverse du temps moyen du centième d’images le plus lent. Les percentiles utilisent le rang supérieur.

| Caméra | Client | FPS moyens | 1 % low | P95 ms | P99 ms | P99,9 ms | CPU en cœurs équivalents | Pic RAM Mio |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| Fixe | Macroquad | 586 | 298 | 2,70 | 3,09 | 3,98 | 2,19 | 129 |
| Fixe | Bevy | 321 | 205 | 3,71 | 4,07 | 7,82 | 2,55 | 537 |
| Mobile | Macroquad | 596 | 292 | 2,52 | 3,08 | 4,51 | 2,05 | 119 |
| Mobile | Bevy | 293 | 179 | 4,50 | 5,25 | 7,22 | 2,42 | 549 |

Le CPU est le temps du processus sur tous ses threads, divisé par le temps mural de la phase mesurée. Les deux clients dépassent 60 FPS à 720p et restent sous 6 Go dans ces scènes. Le client Bevy mobile perd environ 51 % de débit moyen par rapport à Macroquad et son pic RAM augmente d’environ 430 Mio. La scène fixe montre également une augmentation du P99,9. Un passage de 720 images donne peu d’observations pour ce dernier percentile. Macroquad reste disponible.

Un essai solo d’endurance supplémentaire parcourt 24,74 km en 30 000 images sur environ 97 s. Il compte 161 864 évictions, 187 rebasages, 254 meshes au maximum, 1 024 chunks résidents au maximum et zéro image entièrement vide. Les médianes de RAM des quatre quarts successifs sont de 514, 552, 555 et 561 Mio. La hausse ralentit après le premier quart. Cette durée borne le constat et laisse une croissance lente à surveiller sur des sessions plus longues. [Parcours](../benchmarks/results/3b/long-traversal.json) · [Échantillons mémoire](../benchmarks/results/3b/long-rss.json)

## Draws, uploads et coût CPU

En fixe, Macroquad soumet 54 meshes et QuadGl les regroupe en **deux draws indexés**. Le compteur natif de télémétrie est lu sur une image supplémentaire, après la période chronométrée. La somme de ses indices est vérifiée contre les 10 300 quads. Le chemin QuadGl effectue un transfert de sommets et un d’indices par draw, soit quatre mises à jour et 1 771 600 octets de géométrie par image fixe. Le temps mural CPU de préparation des commandes et du flush associé est de 1,45 ms/image en fixe et de 1,27 ms/image en mouvement.

Bevy conserve les meshes côté GPU. Une fois la scène stabilisée, **zéro nouvelle publication de mesh et zéro octet de nouvelle géométrie** dans les fenêtres chronométrées. Le passage de la file de publication prend 0,73 µs/image en fixe et 0,81 µs/image en mouvement. Les mises à jour des ressources du renderer continuent. Une acquisition wgpu distincte, hors benchmark, relève quatre commandes de dessin par image fixe. L’une est un multi-draw indexé dont la limite est 54, deux sont des draws indirects de clustering et la dernière dessine un triangle plein écran. Le nombre effectivement exécuté à l’intérieur du multi-draw dépend du compteur GPU. Sur 24 intervalles stabilisés, la médiane est de 13 appels `Queue::write_buffer`, deux écritures vides ignorées, quatre créations de staging buffer et deux appels `Queue::submit`. Les deux soumissions totalisent 0,287 ms de temps mural médian côté CPU avec le traçage activé. Ces portées de mesure diffèrent entre APIs et ne donnent pas une durée isolée de transfert sur le GPU. [Trace d’une image](../benchmarks/results/3b/wgpu-frame.log) · [24 intervalles](../benchmarks/results/3b/wgpu-audit.json)

## Réseau, compilation et reproduction

Débit médian des entrées de 59,8 à 60,1 Hz suivant le client et le trajet. Les clics du test mixte passent par le serveur et la sauvegarde. Du clic piloté jusqu’à l’observation du mesh à jour chez le pair, les deux éditions Macroquad prennent 59 et 83 ms, celles de Bevy 83 et 117 ms. Quatre actions constituent une preuve fonctionnelle sur boucle locale. Elles ne caractérisent pas la latence sur Internet.

Quatre invocations de Cargo release forcent une recompilation et une liaison. Les premiers passages de 6,52 s pour Macroquad et 12,24 s pour Bevy ont également recompilé des dépendances. Les deux suivants, avec les dépendances en cache, donnent **6,89 s pour Bevy** et **4,82 s pour Macroquad**. Le premier build de la chaîne Bevy et son problème de linker sont détaillés en 3A. [Temps par exécutable](../benchmarks/results/3b/compile.json)

Depuis le dépôt, initialiser l’environnement Rust Windows puis construire en release les paquets `primal-client`, `primal-client-bevy` et `primal-server`. Exécuter `python tools/compare-renderers.py DOSSIER_EXE DOSSIER_NEUF` pour les 16 passages. Exécuter `python tools/test-mixed-renderers.py DOSSIER_EXE AUTRE_DOSSIER_NEUF` pour les éditions, la persistance et les avatars. Ce dernier utilise Pillow pour vérifier les pixels des fenêtres. Les fichiers de résultats conservent les valeurs individuelles. [Données comparatives](../benchmarks/results/3b/comparison.json) · [Éditions](../benchmarks/results/3b/acceptance.json)
