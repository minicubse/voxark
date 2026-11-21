# Forêt moderne sur Ryzen 7 7730U

Windows x64, Ryzen 7 7730U, 16 Go, AMD Radeon (TM) Graphics intégrée, Vulkan et pilote AMD 23.19. Exécutables release, 1280 × 720, FOV vertical 65°, rayon 4 chunks, seed 20261120, attente verticale désactivée. Le client vérifie le nom et le type d’adaptateur. Les essais se suivent dans une seule fenêtre, sans compilation, rendu Blender ni autre client simultané. La seule exception est le scénario multijoueur, mesuré séparément.

## Images du client

| Point de vue | Brut | Palette | Ombres | AO | Low |
|---|---|---|---|---|---|
| A · Clairière | [PNG](../benchmarks/results/3c/A-raw.png) | [PNG](../benchmarks/results/3c/A-palette.png) | [PNG](../benchmarks/results/3c/A-shadows.png) | [PNG](../benchmarks/results/3c/A-ao.png) | [PNG](../benchmarks/results/3c/A-low.png) |
| B · Sous-bois | [PNG](../benchmarks/results/3c/B-raw.png) | [PNG](../benchmarks/results/3c/B-palette.png) | [PNG](../benchmarks/results/3c/B-shadows.png) | [PNG](../benchmarks/results/3c/B-ao.png) | [PNG](../benchmarks/results/3c/B-low.png) |
| C · Belvédère | [PNG](../benchmarks/results/3c/C-raw.png) | [PNG](../benchmarks/results/3c/C-palette.png) | [PNG](../benchmarks/results/3c/C-shadows.png) | [PNG](../benchmarks/results/3c/C-ao.png) | [PNG](../benchmarks/results/3c/C-low.png) |

[Première capture de 3C.1](../benchmarks/results/3c/3c1-clearing.png) · [Même clairière avec AgX](../benchmarks/results/3c/A-agx.png)

À la clairière, la lumière découpe le sol en deux bandes et détache le jeune résineux des troncs du fond. Le grain rompt l’uniformité des grands aplats sans morceler les terrasses. En sous-bois, la capture B rapproche le regard des fougères et de la fleur violette au bord de l’ombre. Les pointes des brins restent anguleuses et les couronnes des différentes espèces partagent encore une même texture. Le belvédère montre le creux de la vallée et les différentes hauteurs de cimes. À 64 blocs de rayon, la brume mange une partie de leurs silhouettes. Le premier plan et le plan intermédiaire sont plus convaincants que le panorama lointain.

AgX et TonyMcMapface utilisent les tables natives de Bevy et la caméra HDR. Dans la clairière A, AgX relève les demi-teintes et désature davantage le sol. Tony garde une séparation plus lisible entre le vert froid à l’ombre et le vert doré au soleil. Les images proviennent toutes du client en fonctionnement.

## Cadence et mémoire

Trois passages de 1 100 images par scénario. L’ordre du second passage est inversé. Une fenêtre d’échauffement d’au moins une seconde et 120 images stables précède chaque mesure. La trajectoire de forêt part du point A et parcourt 3 blocs en X et 1,75 en Z. Le témoin utilise l’exécutable 3B livré et sa trajectoire historique. Le rapport compare des charges de jeu différentes entre la vallée et la forêt. Les écarts entre options de forêt se mesurent sur le même trajet. Les captures et la trace détaillée ont leurs propres passages.

Chaque colonne contient la médiane des trois résultats par passage. Le « 1 % low » est l’inverse du temps moyen des 1 % d’images les plus lentes. Les percentiles sont en millisecondes. La colonne CPU divise le temps CPU total du processus par le temps réel, en équivalents de cœur. La mémoire est le pic de working set Windows. La mémoire vidéo partagée n’est pas isolée.

| Scène | FPS | 1 % low | P95 ms | P99 ms | P99,9 ms | CPU cœurs | Pic Mio |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Vallée 3B | 438,5 | 270,2 | 3,0 | 3,3 | 3,9 | 2,85 | 537 |
| Forêt brute | 377,5 | 223,2 | 3,4 | 3,9 | 4,4 | 2,76 | 543 |
| Brute, double face | 398,9 | 247,3 | 3,3 | 3,8 | 4,3 | 2,82 | 520 |
| Palette + grain | 383,5 | 236,4 | 3,5 | 3,9 | 4,5 | 2,78 | 521 |
| Avec végétation | 355,9 | 227,7 | 3,7 | 4,2 | 4,8 | 2,74 | 816 |
| Lumière, sans ombre | 259,1 | 146,9 | 5,0 | 6,1 | 7,3 | 2,68 | 835 |
| Une cascade | 188,9 | 109,4 | 7,0 | 8,3 | 9,7 | 2,50 | 851 |
| Deux cascades | 164,3 | 109,6 | 7,3 | 8,3 | 9,6 | 2,43 | 852 |
| AO sans ombre | 247,2 | 152,6 | 5,3 | 6,1 | 6,8 | 2,55 | 836 |
| Low complet | 179,4 | 119,0 | 7,0 | 7,7 | 9,0 | 2,38 | 850 |
| Low avec AgX | 182,5 | 113,6 | 6,5 | 7,2 | 10,1 | 2,46 | 851 |

Une cascade de 1 024 pixels sur 44 blocs conserve l’ombre portée des troncs et des reliefs. Passer de zéro à une puis à deux cascades dans la comparaison sans AO réduit la cadence médiane de 259 à 189 puis 164 FPS. Sur les 64 blocs de la scène, le deuxième niveau réduit le débit moyen et les P99 restent pratiquement identiques. Les ombres des plantes restent désactivées. La palette conserve exactement 18 425 quads sur la fin de trajectoire. L’AO en porte le nombre à 30 569 et souligne les contacts au pied des arbres. Le culling est activé pour les faces opaques validées dans les six orientations. Sur ce trajet, le double-face brut a une médiane de 399 FPS contre 377 avec culling. Ce test ne démontre donc aucun gain de cadence dû au culling.

Après cette matrice, la livraison ajoute une borne de végétation pour les empilements artificiels de surfaces. Trois passages supplémentaires sur cet exécutable reproduisent exactement le digest des chunks et quads à chaque répétition. Le profil Low livré atteint **161,4 FPS**, **107,9 FPS en 1 % low**, P95 **7,5 ms**, P99 **8,4 ms**, P99,9 **10,5 ms**. Le pic maximal des trois processus vaut **853 Mio**. Les trois passages figurent dans les fichiers `delivery-run*-low.json`. Les chiffres décrivent cette machine et ces cadrages.

## Géométrie, streaming et transferts

Le nombre de chunks et les triangles correspondent à la fin de la trajectoire. Les quads comptent les surfaces structurelles, les triangles visibles incluent aussi le LOD botanique retenu. Les Mio sont les octets de sommets et indices publiés depuis le lancement, y compris des remplacements pendant le chargement. Les colonnes « tick » et « construction » incluent l’échauffement. Le tick chronomètre le travail synchrone du streamer. La construction chronomètre la production du payload sur le worker, y compris le greedy, les couleurs et les plantes. Les JSON distinguent les publications cumulées et celles de la fenêtre chronométrée.

| Scène | Chunks | Quads | Triangles | Plantes | Publication Mio | Tick ms | Construction ms |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Vallée 3B | 64 | 11257 | 22514 | 0 | — | — | — |
| Forêt brute | 84 | 18425 | 36850 | 0 | 15,8 | 0,39 | 0,10 |
| Brute, double face | 84 | 18425 | 36850 | 0 | 15,8 | 0,39 | 0,10 |
| Palette + grain | 84 | 18425 | 36850 | 0 | 21,3 | 0,41 | 0,25 |
| Avec végétation | 84 | 18425 | 53250 | 995 | 43,1 | 0,43 | 0,33 |
| Lumière, sans ombre | 84 | 18425 | 53250 | 995 | 43,1 | 0,55 | 0,34 |
| Une cascade | 84 | 18425 | 53250 | 995 | 43,1 | 0,60 | 0,37 |
| Deux cascades | 84 | 18425 | 53250 | 995 | 43,1 | 0,63 | 0,36 |
| AO sans ombre | 84 | 30569 | 77538 | 995 | 57,3 | 0,60 | 0,50 |
| Low complet | 84 | 30569 | 77538 | 995 | 57,3 | 0,61 | 0,49 |
| Low avec AgX | 84 | 30569 | 77538 | 995 | 57,3 | 0,58 | 0,50 |

## Soumission graphique

Passages de trace wgpu-core séparés, position A fixe en forêt. Le témoin 3B conserve sa caméra de vallée. Médianes sur 20 intervalles bornés par la passe de clustering de la vue principale. Les commandes indirectes peuvent dessiner plusieurs meshes. Un appel multi-draw compte une fois. Les durées CPU de soumission sont les intervalles entre entrée et retour de Queue::submit avec trace activée. Elles incluent le coût de l’instrumentation et ne mesurent pas le temps GPU. Les écritures de buffers incluent les uniformes et les tampons de scène. Le volume publié dans le tableau précédent décrit les données de maillage, pas l’intégralité du trafic du pilote.

| Scène | Commandes draw | Soumissions | Somme CPU µs/image | Écritures buffers |
| --- | --- | --- | --- | --- |
| Vallée 3B | 4 | 2 | 344 | 13 |
| Forêt brute | 4 | 2 | 342 | 13 |
| Brute, double face | 4 | 2 | 424 | 13 |
| Palette + grain | 4 | 2 | 324 | 13 |
| Avec végétation | 5 | 2 | 336 | 16 |
| Lumière, sans ombre | 7 | 2 | 516 | 16 |
| Une cascade | 8 | 2 | 620 | 22 |
| Deux cascades | 9 | 2 | 849 | 26 |
| AO sans ombre | 7 | 2 | 443 | 16 |
| Low complet | 8 | 2 | 680 | 22 |
| Low avec AgX | 8 | 2 | 682 | 22 |

## Traversée et destruction

La traversée de 6 000 images parcourt 2,47 km en suivant le relief à neuf blocs du sol. Elle atteint 165,1 FPS, un 1 % low de 100,8, P99 de 8,6 ms et P99,9 de 12,1 ms. Le processus culmine à 859 Mio. Le working set médian de la dernière seconde vaut 859 Mio, arrêt compris. Le streamer compte 16016 évictions, 18 changements d’origine et 11145 publications pendant le déplacement, pour 884 Mio de données. 0 image sans chunk visible. Ce compteur ne détecte pas un trou isolé si un autre chunk reste visible.

Des fenêtres Bevy et Macroquad pilotent les actions ordinaires face au même serveur. Le tronc et le feuillage disparaissent dans les deux clients. Le retrait d’un support enlève sa fougère dans le maillage publié par Bevy. Une pierre à la même place reste dépourvue de fougère. Restaurer le support initial la fait revenir. Les clients posent également deux blocs dans des régions distantes avant l’observation de la première confirmation, échangent leurs positions, se reconnectent, puis relisent les révisions après redémarrage. Les 6 éditions instrumentées prennent 93–156 ms dans Bevy et 105–166 ms dans Macroquad, entre commande et observation du maillage publié, réseau local et ordonnancement inclus.

[Fougère avant](../benchmarks/results/3c/network-fern-before.png) · [Support retiré](../benchmarks/results/3c/network-fern-removed.png) · [Fougère restaurée](../benchmarks/results/3c/network-fern-restored.png)

[Résultats multijoueurs](../benchmarks/results/3c/acceptance.json) · [Traversée et mémoire brutes](../benchmarks/results/3c/traversal.json)

## Reproduire

Compiler le workspace en release avec le verrou Cargo. Pour le témoin, conserver l’exécutable Bevy de l’archive 3B, SHA-256 `b2977c9f7948eb0c32d58bbcd638ea3eea642db5da0797c66bfc51005feadace`. Depuis la racine, remplacer les deux chemins par ceux des exécutables. Les mesures utilisent Python avec psutil. La vérification d’images multijoueur utilise aussi Pillow.

```powershell
$mesure = @('--client','CHEMIN/client-3C.exe','--baseline','CHEMIN/client-3B.exe','--output','resultats-3c')
python tools/benchmark-forest.py measure @mesure
python tools/benchmark-forest.py trace @mesure
python tools/benchmark-forest.py traverse @mesure
python tools/benchmark-forest.py capture @mesure
python tools/test-mixed-renderers.py DOSSIER_BINAIRES NOUVEAU_DOSSIER --forest
```

Les caméras et réglages exacts sont centralisés dans le [script de mesure](../tools/benchmark-forest.py). Les JSON individuels conservent les frametimes de forêt, les échantillons de mémoire, l’adaptateur, les options et les compteurs bruts. Le binaire historique produit les agrégats disponibles en 3B. Un extrait wgpu natif accompagne chaque variante dans le [dossier de résultats](../benchmarks/results/3c/).
