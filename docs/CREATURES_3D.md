# Wyverne animée dans la forêt

Extrayez le ZIP Windows 3D et ouvrez **Inspecter-Wyverne.cmd**. Le client Bevy place une wyverne dans la clairière et lance la séquence complète. Vous pouvez marcher en caméra libre tout au long de la démonstration. La forêt du jalon 3C, le mode de jeu **Jouer-Foret.cmd**, les deux clients réseau et les sauvegardes restent accessibles avec leurs lanceurs habituels.

| Contrôle d’inspection | Action |
|---|---|
| Clic gauche, souris, Échap | Capturer, orienter puis libérer la souris |
| ZQSD (AZERTY) ou WASD (QWERTY), Espace, Ctrl gauche, Maj | Déplacer, monter, descendre, accélérer la caméra |
| Molette | Avancer ou reculer le long de la visée |
| 0 | Relancer la séquence complète dans la clairière |
| 1, 2, 3, 4, 5, 6 | Repos, marche, décollage, vol battu, plané, atterrissage |
| P, R | Pause/reprise, réinitialisation de la sélection |
| Touches physiques [ et ] | Ralentir ou accélérer l’horloge d’animation |

Sous Windows, deux captures successives pendant la pause montrent la même pose. Espace déplace alors la caméra de 27,80 à 32,67 m d’altitude. Après reprise, deux nouvelles captures montrent de nouveau un mouvement.

Les six clips sont sur place dans le modèle. La racine de présentation suit indépendamment un parcours répété à l’identique qui finit au sol. Le mouvement utilise des vitesses continues aux raccords et un fondu natif de 220 ms entre clips. Le repos, la marche, le battement et le plané bouclent. Le décollage et l’atterrissage se jouent une fois. La position mondiale reste en double précision jusqu’à sa conversion en repère local du renderer. Pendant la pause d’animation, la caméra utilise le temps réel.

Pour tester quatre variantes depuis un terminal Windows :

~~~bat
Jouer-Bevy.cmd --preset forest-modern --creatures 4 --creature-state flap --creature-placement near
~~~

Les variantes Jade, Braise et Obsidienne alternent par instance. La création d’un acteur instancie sa hiérarchie de joints et son lecteur. Les quatre maillages, les matrices de bind, les clips, le graphe et les matériaux d’une même palette restent partagés. Les membranes emploient le matériau glTF double face. Le réglage `--creature-shadows off` coupe uniquement les ombres portées des primitives de la créature. Le réglage `--membrane-shadows off` préserve les ombres du corps et des autres primitives.

[Voir la séquence complète dans le client Bevy · 18,2 s, 720p](../benchmarks/results/3d/flight-sequence.mp4) · [Voir Jade en gros plan](../benchmarks/results/3d/jade.png).

La vidéo assemble 455 captures du client à 25 images par seconde. Seul l’export explicite `--creatures 1 --creature-film DOSSIER --frames 455` utilise une caméra de suivi. L’ouverture du lanceur garde la caméra libre. La séquence à vitesse normale et sa répétition à vitesse double parcourent les six états puis reviennent au repos. Le test compare la direction tête–torse issue des joints transformés à l’avant de la racine, puis à la direction réelle du trajet. Les deux lectures conservent les échelles de joints et montrent deux clips actifs durant les transitions. [Résultat](../benchmarks/results/3d/sequence-validation.json) · [Mesures de pose à vitesse normale](../benchmarks/results/3d/sequence-native-1x.json) · [Mesures au double de la vitesse](../benchmarks/results/3d/sequence-native-2x.json).

## Remplacer le modèle provisoire

Le modèle livré a été produit procéduralement pour valider le pipeline. Il possède une géométrie, des poids et des pistes d’animation réels. Le futur modèle artistique de GLM apportera sa sculpture, ses textures éventuelles et son jeu d’animations final. Préparer son manifeste et le fichier GLB, ou le glTF et ses ressources relatives, sous `assets/creatures/`. Le client accepte `--creature-manifest creatures/autre.json` et `--asset-root CHEMIN`. Il charge le modèle, découvre les clips et remplace les matériaux par nom de slot.

Prendre [le manifeste livré](../assets/creatures/wyvern.json) comme contrat. La scène est en mètres, +Y vers le haut, +Z vers le museau. Le pivot est au sol sous le corps. La conversion native de Bevy 0.20.1 tourne l’entité de scène vers −Z. La correction des sommets reste désactivée. Le contrôleur applique ensuite l’orientation du déplacement à l’acteur. Indiquer explicitement l’échelle, la scène, les six correspondances de clips et les quatre slots. Les matrices de bind et les indices/poids des articulations sont vérifiés au chargement. Le manifeste ou une dépendance invalide produit un diagnostic et un code de sortie d’échec.

[Construction, anatomie et contrôles du GLB](WYVERN_ASSET.md) · [Essais d’import réels, y compris glTF avec buffer externe](../benchmarks/results/3d/import-validation.json).

## Démonstration locale et données serveur futures

La scène de développement utilise le terrain de forêt commun et un acteur de présentation local. Le serveur actuel simule les joueurs et leurs éditions. Au prochain jalon de réplication, l’identifiant et l’espèce identifieront chaque créature. Une position et une orientation horodatées, un mode de déplacement et une vitesse permettront l’interpolation. L’état d’animation logique, son instant de départ et la vitesse de lecture permettront de retrouver la phase au retour dans la zone d’intérêt. Réserver l’état de vie au serveur. Le choix des maillages, les matrices de pose, les matériaux de palette, les ombres et les LOD appartiennent au client.

## Validation Windows et performances

La démonstration a été mesurée sur Ryzen 7 7730U, 16 Go et Radeon intégrée, avec Vulkan et le pilote AMD 23.19. Scène éclairée Low 3C, 1280 × 720, seed 20261120, rayon 4, caméra fixe au point A, VSync désactivée. Trois répétitions de 1 100 images après stabilisation, ordre inversé au deuxième passage. Les modes de trace et de capture ont des essais séparés. Chaque colonne présente la médiane de ses trois passages. Le 1 % low utilise la durée moyenne des images du centile le plus lent.

| Scène | FPS | 1 % low | P95 ms | P99 ms | P99,9 ms | CPU en cœurs | Pic RSS Mio | Animation µs |
|---|---|---|---|---|---|---|---|---|
| 3C livré, forêt seule | 181,2 | 100,2 | 7,1 | 8,6 | 10,8 | 2,42 | 850 | — |
| 3D, forêt seule | 182,0 | 103,2 | 6,8 | 8,7 | 10,2 | 2,59 | 859 | 274 |
| 1 · proche · sans ombres | 185,9 | 117,3 | 6,7 | 7,9 | 8,8 | 2,76 | 868 | 431 |
| 1 · proche · ombres | 180,2 | 89,9 | 7,3 | 10,1 | 12,4 | 2,76 | 869 | 415 |
| 4 · proche · sans ombres | 177,4 | 106,8 | 7,1 | 8,5 | 10,3 | 2,83 | 616 | 464 |
| 4 · proche · ombres | 168,4 | 90,5 | 8,0 | 10,1 | 11,5 | 2,69 | 622 | 453 |
| 12 · proche · sans ombres | 156,4 | 86,1 | 9,1 | 10,3 | 12,9 | 2,59 | 604 | 492 |
| 12 · proche · ombres | 160,9 | 84,1 | 8,4 | 10,6 | 13,3 | 2,70 | 603 | 504 |
| 12 · loin · sans ombres | 153,4 | 86,6 | 9,4 | 11,0 | 11,8 | 2,76 | 865 | 543 |
| 12 · loin · ombres | 159,3 | 82,6 | 8,6 | 10,0 | 13,2 | 2,64 | 605 | 480 |
| 12 · hors champ · sans ombres | 177,9 | 101,4 | 7,1 | 9,2 | 10,8 | 2,77 | 612 | 463 |
| 12 · hors champ · ombres | 172,1 | 98,0 | 7,6 | 9,6 | 11,1 | 2,67 | 867 | 502 |

La colonne animation encadre le groupe natif AnimationSystems dans PostUpdate. Ce temps mural comprend ordonnancement et travail concurrent éventuel. Le nombre de cœurs est le temps CPU du processus divisé par la durée réelle. Le pic RSS vient du working set Windows, échantillonné toutes les 120 ms avec son maximum système. Entre une et douze instances proches avec ombres, la différence des médianes de RSS divisée par onze est de -24,15 Mio par instance. Cette pente dépend de l’allocation globale et du comportement du pilote. Elle ne mesure pas la taille exacte d’un acteur.

| Instances | Primitives skinnées | Joints vivants | Triangles instanciés | Octets maillages uniques | Octets bind partagés | Octets références joints | Chargement ms | Publication ms |
|---|---|---|---|---|---|---|---|---|
| 1 | 4 | 23 | 6048 | 254080 | 1472 | 736 | 17,80 | 2,28 |
| 4 | 16 | 92 | 24192 | 254080 | 1472 | 2944 | 17,39 | 1,96 |
| 12 | 48 | 276 | 72576 | 254080 | 1472 | 8832 | 17,86 | 1,74 |

Les métriques dédupliquent les handles du moteur. Les quatre primitives contiennent ensemble 254 080 octets de sommets et indices et partagent une table de bind de 1 472 octets. Les six clips utilisent 29,440 octets de buffers source distincts dans le GLB. Le GLB n’utilise aucune texture externe. Un graphe dessert tous les lecteurs. Chaque peau de primitive conserve sa liste d’entités de joints. Chaque créature a 23 entités articulées et une pose indépendante. Les coûts de la mémoire vidéo partagée, des allocations privées du pilote et des courbes décodées ne sont pas isolés par ces compteurs.

Le chargement s’arrête quand le manifeste et toutes ses dépendances sont disponibles. La publication va de la découverte des primitives skinnées dans le monde principal jusqu’à la présence des quatre RenderMesh dans le monde de rendu. C’est une latence CPU observable entre deux systèmes. Le chronométrage ne constitue pas une mesure de bande passante PCIe.

| Instances proches avec ombres | Commandes draw | Soumissions | CPU soumission µs | Écritures buffers | GPU passe opaque ms | GPU passe ombres ms |
|---|---|---|---|---|---|---|
| 0 | 8 | 2 | 657 | 22 | 2,68 | 0,36 |
| 1 | 12 | 2 | 610 | 41 | 2,50 | 0,37 |
| 4 | 12 | 2 | 720 | 41 | 3,09 | 0,46 |
| 12 | 12 | 2 | 668 | 37 | 3,02 | 0,40 |

Les appels et durées de soumission viennent de 20 intervalles wgpu-core par variante. Une commande multi-draw compte une fois. Les horodatages GPU de Bevy incluent la forêt et les créatures. Le skinning est intégré aux shaders de dessin et d’ombres. Il n’existe pas de passe chronométrée autonome pour ce shader. Les différences entre les nombres d’instances indiquent le coût combiné dans ces scènes.

**Décision de LOD.** Le modèle provisoire a un seul niveau de géométrie, 6 048 triangles, que le moteur peut réutiliser pour toutes les instances. Le culling par vue de Bevy écarte les dessins hors champ. L’horloge logique et les poses restent actives, y compris derrière la caméra. Sur douze acteurs hors champ avec ombres, le groupe d’animation prend 502 µs par image, contre 274 µs dans le client 3D sans acteur. Nous conservons les transitions natives continues. Un modèle GLM plus lourd justifierait un nouveau test avec un LOD1 réellement décimé conservant les joints et une représentation LOD2 distincte à longue distance. Aucun bénéfice de ces deux représentations n’est revendiqué ici.

[Repos](../benchmarks/results/3d/idle-side.png) · [Marche](../benchmarks/results/3d/walk-side.png) · [Décollage](../benchmarks/results/3d/takeoff-side.png) · [Battement, face](../benchmarks/results/3d/flap-front.png) · [Battement, dos](../benchmarks/results/3d/flap-back.png) · [Plané](../benchmarks/results/3d/glide-side.png) · [Atterrissage](../benchmarks/results/3d/land-side.png) · [Trois palettes](../benchmarks/results/3d/near-4-palettes.png).

La membrane reste visible de face et de dos. L’ombre portée change avec le battement et se superpose aux ombres du terrain. Le profil sombre et les ailes croisées en perspective rendent certaines formes moins lisibles dans le sous-bois. Le modèle provisoire privilégie la déformation et les articulations. [Ombres des créatures activées](../benchmarks/results/3d/near-12-shadow-on.png) · [Même scène sans ces ombres](../benchmarks/results/3d/near-12-shadow-off.png).

À caméra et pose identiques, couper seulement la projection des membranes laisse celles-ci visibles et modifie 17 143 pixels de plus de cinq niveaux RGB. [Membranes projetant leur ombre](../benchmarks/results/3d/membrane-shadow-on.png) · [Projection des membranes coupée](../benchmarks/results/3d/membrane-shadow-off.png).

Le test d’instance modifie le matériau Jade, contrôle les teintes Braise et Obsidienne, rétablit les couleurs, puis supprime un acteur et vérifie la disparition de son squelette. Les trois acteurs restants conservent leurs matériaux partagés. Le test caméra mesure des transformations natives aux poignets, à la queue, à la mâchoire et à la tête pendant plusieurs secondes hors champ. [Isolation et nettoyage](../benchmarks/results/3d/lifecycle.json) · [Retour en vol après rotation de caméra](../benchmarks/results/3d/probe-offscreen-return.json).


Le contrôle de livraison à douze wyvernes dénombre douze instances dans le champ de la caméra principale et 276 joints. Les transformations locales et globales occupent au minimum 2 576 octets par instance, auxquels s’ajoutent 736 octets de références aux joints. Les quatre maillages totalisent 254 080 octets partagés et les matrices de bind 1 472 octets. Cette passe indépendante donne 162,1 FPS et 10,19 ms au P99. [Contrôle du client livré](../benchmarks/results/3d/delivery-12.json).

Les mesures brutes et les captures sont dans le [dossier du jalon](../benchmarks/results/3d/). La référence 3C publiée utilisait un léger déplacement de caméra. La série 3D réexécute l’exécutable 3C sur le cadrage fixe commun pour isoler les créatures.

### Reproduire

Compiler le workspace Windows x64 en release. Les commandes suivantes se lancent depuis la racine des sources avec Python et psutil. Le contrôle des fichiers GLB utilise la bibliothèque standard.

~~~powershell
python tools/validate-wyvern.py
python tools/test-creature-import.py CHEMIN/client-3D.exe --result import.json
$op = @('--client','CHEMIN/client-3D.exe','--baseline','CHEMIN/client-3C.exe','--asset-root','assets','--output','resultats-3d')
python tools/benchmark-creatures.py measure @op
python tools/benchmark-creatures.py profile @op
python tools/benchmark-creatures.py trace @op
python tools/benchmark-creatures.py capture @op
python tools/benchmark-creatures.py probe @op
python tools/benchmark-creatures.py summarize --output resultats-3d
~~~
