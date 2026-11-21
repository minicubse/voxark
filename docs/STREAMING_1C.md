# Streaming prédictif 1C sur Ryzen 7 7730U

Sur le vol de 4,22 km au rayon de préchargement 2, le sol sous la trajectoire manque pendant **244 → 4 images sur 720** en médiane. Les trois passages de la nouvelle version donnent chacun quatre images manquantes. La plus longue interruption médiane tombe de 15 images à une. Sur le vol de 8,44 km, le bilan passe de 350 à 5 images sur 1 440. Le gain s’accompagne d’une hausse du temps CPU sur ces parcours. La téléportation reste le cas où le sol arrive plus tard avec le nouveau plan.

## Mesurer avant de changer

La version distribuée 1B (`0b05e67`) redonne 126 images sans sol avec son ancien indicateur. Celui-ci cherchait la plus haute cellule solide **déjà chargée**. Il pouvait donc créditer une couche inférieure. La nouvelle sonde de benchmark reconstruit une colonne de référence dans un espace temporaire et utilise les éditions résidentes lorsqu’elles sont connues. Elle distingue l’absence du chunk de surface et l’absence de géométrie non vide installée pour ce chunk. Elle échantillonne la colonne sous la position suivie, une fois par image. Elle ne compte ni les pixels manquants ailleurs à l’écran ni les éditions encore inconnues sur disque. [Mesure originale](../benchmarks/results/1c/original/) · [Contrôle instrumenté](../benchmarks/results/1c/audit/)

Sur le diagnostic du plan 1B, 21 images manquent de données et 215 ont déjà les données sans maillage installé. Dans les trois contrôles A du vol au rayon 2, la médiane du temps par génération est de 0,019 ms, la lecture régionale de 0,210 ms et le meshing de 0,055 ms. Un résultat de worker terminé attend environ 8,71 ms avant traitement. Le premier essai de priorité prédictive ramène les trous à zéro, mais invalide 2 883 mailles. Attendre les voisins demandés avant de mailler les chunks moins urgents ramène les mailles obsolètes à 229 en médiane dans la version livrée. Une passe de diagnostic redimensionnée à 1536 × 793 a été écartée des mesures A/B. [Essai sans attente des voisins](../benchmarks/results/1c/pilot-2/) · [Passe écartée](../benchmarks/results/1c/halo-2/)

## Protocole

60 processus Windows en release, un à la fois, trois répétitions par variante et par cas, ordre A/B alterné. A est le plan 1B avec les sondes communes et les mêmes parcours au commit `ca45b99`. B est le binaire livré. Les deux utilisent le greedy scalaire, Macroquad, la seed 20261120, 1280 × 720, le même pilote Radeon 31.0.21925.1001 et le même Ryzen 7 7730U. Les fenêtres mesurées excluent 120 images d’échauffement. Chaque lancement dispose d’un monde régional neuf. Les revisites s’effectuent dans le même processus. CPU, mémoire, maxima des files, évictions et obsolescences couvrent tout le lancement. L’ordre alterné réduit les biais d’ordre, mais trois répétitions sur un seul poste ne suffisent pas pour généraliser les petites différences.

Le rayon du tableau est celui de l’anticipation. Les rayons de chargement, de rendu et de simulation restent respectivement à 8, 7 et 1. Quatre workers, 24 travaux en vol, 12 publications de maillages par tick, 64 Mio de résidence estimée (1 024 chunks effectifs), 96 Mio de préparation et 160 Mio de cache de géométrie. Le budget du fil principal de 4 ms est coopératif. Chaque cellule ci-dessous donne la médiane des trois passages pour A puis pour B. Les percentiles sont calculés sur les images d’un passage avant de prendre la médiane des répétitions. [Mesures individuelles et étapes P50/P95](../benchmarks/results/1c/runs.csv) · [Traces compressées, résultats, empreintes et journaux](../benchmarks/results/1c/ab/)

## Disponibilité et fluidité

« Sol » compte les images sans géométrie du chunk de surface, « suite » la plus longue série et « collision » les images où les cellules du corps ou des appuis sont indisponibles. Les parcours de marche, sprint et vol normal utilisent le joueur physique. Les vols extrêmes et téléportations imposent la position pour éprouver le streamer. Les huit parcours ont respecté leurs contrôles. Les moyennes FPS décrivent la cadence de la boucle, y compris la présentation.

| Parcours | Rayon | Sol (images) | Suite | Collision | FPS | P95 ms | P99 ms | P99,9 ms |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Marche | 2 | 0 → 0 | 0 → 0 | 0 → 0 | 105,3 → 108,6 | 12,4 → 12,2 | 39,7 → 13,0 | 51,7 → 14,3 |
| Sprint | 2 | 0 → 0 | 0 → 0 | 0 → 0 | 114,8 → 102,1 | 11,8 → 13,3 | 14,0 → 16,0 | 17,1 → 21,1 |
| Vol normal | 2 | 0 → 0 | 0 → 0 | 0 → 0 | 119,0 → 118,3 | 9,8 → 10,4 | 11,2 → 11,8 | 14,7 → 14,4 |
| Vol 4,22 km | 2 | 244 → 4 | 15 → 1 | 0 → 0 | 120,0 → 119,1 | 9,9 → 10,3 | 10,3 → 12,2 | 11,6 → 16,8 |
| Vol 4,22 km | 4 | 242 → 3 | 17 → 1 | 0 → 0 | 119,8 → 120,0 | 10,2 → 10,0 | 11,3 → 10,5 | 15,6 → 11,9 |
| Vol 4,22 km | 8 | 146 → 0 | 11 → 0 | 0 → 0 | 120,0 → 120,0 | 9,3 → 9,7 | 9,6 → 10,1 | 10,2 → 11,5 |
| Téléportation A/R | 2 | 5 → 7 | 3 → 4 | 2 → 2 | 119,3 → 119,0 | 10,3 → 10,2 | 10,9 → 11,7 | 13,0 → 13,0 |
| Retour en région | 2 | 0 → 0 | 0 → 0 | 0 → 0 | 120,0 → 114,9 | 9,3 → 11,6 | 9,7 → 13,4 | 11,2 → 15,3 |
| Éditions en marche | 2 | 0 → 0 | 0 → 0 | 0 → 0 | 118,0 → 118,9 | 10,1 → 10,3 | 12,5 → 11,6 | 13,2 → 12,4 |
| Vol 8,44 km | 2 | 350 → 5 | 18 → 2 | 0 → 0 | 120,0 → 119,8 | 9,9 → 10,2 | 10,3 → 11,0 | 10,8 → 15,3 |

Aucune des neuf répétitions B de marche, sprint ou vol normal n’a présenté de lacune sous le joueur. A obtenait déjà ce résultat. La médiane du sprint passe de 114,8 à 102,1 FPS et son P99 de 14,0 à 16,0 ms. La téléportation présente 7 images sans sol sur chacun des trois passages B, contre 5 sur chacun des trois A. Les deux variantes ont deux images sans données de collision après le repositionnement scripté. Un joueur physique attend à la frontière inconnue puis reprend son mouvement sur les données réelles.

## Charge et mémoire

« CPU » additionne les temps processeur des threads du processus, en secondes. « CPU obsolète » mesure le temps des workers pour les travaux rejetés, en millisecondes. Sous Windows, les incréments observés sont de 15,625 ms. Un total nul ne permet donc pas de conclure qu’un travail rejeté était gratuit. « Générés/s » compte les événements de génération reçus dans la fenêtre mesurée par seconde de temps réel. Ce débit dépend aussi de la demande sur le parcours. Les plafonds configurés sont identiques pour A et B.

| Parcours | Rayon | Loads obsolètes | Mailles obsolètes | CPU obsolète ms | CPU s | Pic chunks | Pic RSS Mio | Générés/s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Marche | 2 | 1 → 0 | 154 → 10 | 0,0 → 0,0 | 4,58 → 4,78 | 1024 → 1024 | 579,6 → 318,7 | 172 → 28 |
| Sprint | 2 | 1 → 0 | 170 → 10 | 0,0 → 0,0 | 5,72 → 4,81 | 1024 → 1024 | 535,5 → 354,3 | 219 → 54 |
| Vol normal | 2 | 6 → 0 | 138 → 10 | 0,0 → 0,0 | 4,25 → 4,30 | 1024 → 1024 | 352,6 → 354,2 | 180 → 31 |
| Vol 4,22 km | 2 | 1 → 14 | 767 → 229 | 15,6 → 0,0 | 6,50 → 7,34 | 715 → 1024 | 338,4 → 346,9 | 897 → 1605 |
| Vol 4,22 km | 4 | 2 → 14 | 574 → 95 | 0,0 → 0,0 | 6,77 → 7,45 | 734 → 1024 | 305,8 → 350,7 | 937 → 1620 |
| Vol 4,22 km | 8 | 603 → 197 | 475 → 435 | 140,6 → 31,2 | 6,42 → 8,75 | 736 → 1024 | 311,5 → 342,5 | 927 → 1314 |
| Téléportation A/R | 2 | 19 → 0 | 450 → 31 | 0,0 → 0,0 | 4,11 → 4,08 | 841 → 1024 | 345,3 → 318,4 | 516 → 724 |
| Retour en région | 2 | 1 → 27 | 739 → 196 | 15,6 → 0,0 | 6,00 → 6,61 | 907 → 1024 | 530,9 → 348,9 | 513 → 720 |
| Éditions en marche | 2 | 6 → 0 | 151 → 10 | 0,0 → 0,0 | 4,56 → 4,36 | 1024 → 1024 | 355,1 → 352,5 | 178 → 31 |
| Vol 8,44 km | 2 | 5 → 6 | 2105 → 332 | 0,0 → 0,0 | 11,34 → 14,22 | 734 → 1024 | 346,3 → 351,2 | 899 → 1703 |

Le pic observé sur l’ensemble des passages B atteint 24 travaux en vol, 24 en attente des workers, 1 024 chunks résidents et 588,6 Mio de working set. Sur la longue traversée, le CPU médian passe de 11,34 à 14,22 s pour le même nombre d’images. Au rayon 8, B obtient zéro lacune en médiane au prix de 197 chargements et 435 mailles obsolètes. Le rayon 2 reste le réglage par défaut. Le parcours de 8,44 km et le test headless à positions extrêmes vérifient les bornes observées. Une fuite lente sur plusieurs heures demanderait une mesure plus longue.

## Latences, uploads et persistance

| Étape sur 4,22 km, rayon 2 | P50 par passage A → B (ms) | P95 A → B (ms) |
|---|---:|---:|
| Sélection du plan, ticks réutilisant le plan inclus | 0,0008 → 0,0014 | 0,4764 → 0,6426 |
| Soumission des travaux | 0,0240 → 0,0666 | 0,1370 → 0,2527 |
| Attente avant lecture | 0,3371 → 0,5098 | 0,7431 → 1,0607 |
| Lecture régionale | 0,2098 → 0,1916 | 0,4404 → 0,3594 |
| Génération | 0,0185 → 0,0187 | 0,0301 → 0,0316 |
| Attente avant meshing | 0,0819 → 0,0455 | 0,5795 → 0,8433 |
| Meshing | 0,0545 → 0,0706 | 0,1575 → 0,1714 |
| Préparation des buffers | 0,0019 → 0,0009 | 0,0241 → 0,0304 |
| Résultat prêt → réception | 8,7125 → 7,6567 | 16,5380 → 9,7089 |
| Publication CPU du streamer | 0,0239 → 0,0406 | 0,0502 → 0,0701 |
| Adoption par le client | 0,0059 → 0,0054 | 0,0171 → 0,0129 |
| Soumission de la scène | 0,0261 → 0,0791 | 0,4219 → 0,7191 |
| Flush terrain, uploads et appels de dessin | 0,0347 → 0,0802 | 0,3502 → 0,6095 |
| Sonde de collision | 0,0010 → 0,0012 | 0,0021 → 0,0026 |
| Éviction et examen des sauvegardes | 0,0497 → 0,1041 | 0,1532 → 0,2960 |
| Examen des annulations | 0,0065 → 0,0048 | 0,0122 → 0,0130 |

Le temps de la boucle de démarrage jusqu’à une colonne d’apparition chargée et un joueur utilisable passe de 10,8 à 26,5 ms en médiane sur les trente lancements de chaque variante. Le CSV inclut aussi ce temps pour chaque passage, le nombre de données ou mailles absentes, le compteur 1B, la distance, les pics de file et les latences des éditions.

Sur le parcours d’édition en mouvement, les six passages ont relu les 12 éditions attendues après sauvegarde, sans édition en attente. Les P95 médians du délai commande → maille sont **17,7 → 19,2 ms** et ceux du délai commande → confirmation disque **66,5 → 66,6 ms**. Une des trois passes B atteint 165,6 ms au P95 disque. Le test Windows exerce aussi le clic, F5, la fermeture et la réouverture d’un ancien monde.

Pour isoler les uploads, une compilation diagnostique a chronométré les deux appels de mise à jour de buffers par lot dans Macroquad 0.4.16. Sur un passage 4,22 km, 2 520 flushes non vides (terrain et interface, échauffement inclus) représentent 40 246 appels et 1 002,35 Mio soumis. Le temps CPU des uploads par flush est de 0,055 ms en médiane et 1,40 ms au P95. Le diagnostic écrit une trace par flush et tourne séparément de l’A/B. La durée d’exécution sur le GPU et la VRAM dédiée ne sont pas mesurées. Le client distribué utilise la dépendance standard. [Mesure des uploads](../benchmarks/results/1c/uploads/) · [Sonde reproductible](../tools/profile-uploads.py)

## Reproduction et suite

Construire le contrôle avec `cargo build --locked --release -p primal-client` sur `ca45b99`, puis construire la version livrée depuis la révision 1C dans une autre copie. Exécuter sous PowerShell :

```powershell
./tools/streaming-ab.ps1 -Control C:/bench/control.exe -Predictive C:/bench/primal-client.exe
python tools/summarize-streaming.py benchmarks/results/1c
python tools/profile-uploads.py benchmarks/results/1c/uploads
```

L’exécutable Windows livré porte le SHA-256 `75F30CED628DD472A18FC48A8B6C29A98751F0D49B396013C0E4F3F35A60C225`, identique sur les trente passages B. Une réédition de liens GNU peut modifier l’horodatage PE et sa somme de contrôle.

L’agrégateur accepte les traces CSV brutes ou compressées. Les tests de révision, d’éviction, de relecture concurrente, de stockage régional et d’import historique restent dans le workspace. Les quatre nouveaux tests portent sur les changements de direction et de destination, la chute vers une couche absente, la progression d’un chunk de fond pendant des éditions et les deux joueurs distants sous budget partagé. Le prochain jalon peut partir de `tick_views` et de la simulation indépendante du rendu. [Architecture](ARCHITECTURE.md) · [Critères du premier serveur](ROADMAP.md#premier-serveur-autoritaire) · [Audit et licences Horizon](HORIZON_AUDIT.md)
