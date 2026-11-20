# Mesures du 20 novembre 2026

Windows 11 build 26200, Ryzen 7 7730U, Radeon intégrée, pilote 31.0.21925.1001, 15,38 Gio de RAM utilisable et écran 60 Hz. Build release, Rust 1.100.0, Macroquad 0.4.16, Miniquad 0.4.11. Cible client initiale : 60 FPS stables à 720p, 30 FPS à 1080p, mémoire inférieure à 6 Go.

## Parcours graphique

Vallée de seed 20261120, 256 chunks résidents, 151 avec un maillage. Même caméra mobile, champ vertical 65°, distance 112 blocs, 60 ticks de simulation par seconde simulée. Chaque passage exécute 1 320 frames. Les 120 premières servent à la chauffe et comprennent une destruction et une repose sauvegardées. Les 1 200 suivantes fournissent les mesures. Chaque combinaison a été exécutée deux fois, avec quatre processus successifs par série. L'ordre de la seconde série est inversé. Les fichiers JSON contiennent le hash du binaire par série dans les métadonnées machine.

| Résolution | Mesher | FPS moyens A / B | 1 % low A / B | 0,1 % low A / B | P99 en ms A / B | Pic résident en Mio A / B |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| 1280 × 720 | Faces | 86,6 / 116,9 | 50,1 / 58,9 | 46,8 / 53,8 | 18,47 / 16,07 | 356 / 347 |
| 1280 × 720 | Greedy | 100,9 / 113,9 | 54,1 / 57,1 | 46,8 / 55,0 | 17,63 / 17,04 | 327 / 342 |
| 1920 × 1080 | Faces | 104,8 / 114,1 | 57,3 / 58,4 | 50,0 / 52,9 | 16,43 / 16,07 | 615 / 645 |
| 1920 × 1080 | Greedy | 116,1 / 113,9 | 58,0 / 57,9 | 54,0 / 54,9 | 16,45 / 16,60 | 376 / 376 |

Les FPS décrivent les intervalles entre frames du client. Un écran 60 Hz peut présenter au plus 60 rafraîchissements par seconde. Les lows sont les inverses des moyennes des 12 frames et des 2 frames les plus lentes respectivement. Le 0,1 % low reste sensible à un seul retard. Le pic mémoire correspond au working set du processus relevé sous Windows. Consulter les [résultats bruts avec P50, P95, P99, maximum et journaux fonctionnels](../benchmarks/results/2026-11-20/).

Le mode greedy passe de 73 678 à 24 758 quads sur la scène jouable. Les deux séries montrent un pic résident plus faible. L'avantage de FPS moyens change selon la série. Le choix par défaut repose sur la réduction de géométrie et de mémoire. Le mode `--mesher faces` reste disponible. Le seuil de 30 FPS en 1080p est dépassé sur ce trajet. Les deux 1 % lows 720p du mode par défaut restent sous 60 FPS. Ce scénario court ne couvre ni les grandes bases ni les montures ni le serveur.

## Génération et maillage CPU

Seed 42, monde 128 × 64 × 128 en mémoire, dix répétitions release. Médianes rapportées par le banc (échantillon central supérieur).

| Opération | Médiane | Maximum | Géométrie |
| --- | ---: | ---: | ---: |
| Génération | 1,68 ms | 1,86 ms | 348 616 cellules solides |
| Cubes complets | 65,41 ms | 77,58 ms | 2 091 696 faces |
| Faces exposées | 6,80 ms | 7,22 ms | 71 624 quads |
| Greedy | 22,73 ms | 24,68 ms | 23 443 quads |

Le greedy consomme davantage de CPU à la construction du maillage complet. Les deux copies de la grille occupent exactement 2 097 152 octets de données, hors index, édits et maillages. La comparaison de parcours à stockage constant donne 12,38 ms par régions de 16³, 12,28 ms en 32³ et 16,25 ms en 64³. Changer réellement la taille des chunks de stockage et mesurer la latence d'édition reste un essai distinct. [Résultats CPU](../benchmarks/results/2026-11-20/voxel.txt)

## Persistance

Onze remplacements synchronisés et relectures par taille. Les relectures comprennent la régénération. Chaque relevé est suivi d'une comparaison complète des modifications. Les variations de synchronisation disque expliquent l'intérêt de répéter ces essais.

| Modifications | Fichier | Sauvegarde médiane / max | Relecture médiane / max |
| ---: | ---: | ---: | ---: |
| 1 | 28 octets | 8,67 / 8,97 ms | 1,77 / 1,88 ms |
| 10 000 | 40 024 octets | 4,81 / 5,16 ms | 2,55 / 2,79 ms |
| 65 536 | 262 168 octets | 6,34 / 10,08 ms | 7,19 / 7,67 ms |

La barrière disque et le remeshing synchrone sont des coûts observés sur le chemin d'édition. Pour identifier la cause des frames lentes en déplacement, instrumenter séparément le temps de soumission et le temps GPU, puis répéter un scénario long avec éditions. [Résultats disque](../benchmarks/results/2026-11-20/storage.txt)
