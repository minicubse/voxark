# Primal Voxel — état du projet

## Jalon 4B.1 intégré — première wyverne réactive

La [PR #5](https://github.com/minicubse/voxark/pull/5) est intégrée à `main` au commit `061bc22`. Le serveur démarre en mode `demo`. L’option `--creature-behavior reactive` ou le champ JSON `"creature_behavior": "reactive"` active l’IA de la première wyverne. Elle détecte les joueurs à portée même derrière un mur. Un obstacle peut bloquer son déplacement et son retour. [Comportement, limites et validation sur PC 1](JALON_4B1.md)

## Jalon 4A intégré — wyverne autoritaire

Le jalon 4A et la wyverne artistique sont intégrés à `main` au commit `9524772`. Le serveur pilote les déplacements et les six états. Le client Bevy v2 restitue les créatures et le client Macroquad v1 continue de recevoir le terrain et les joueurs. Les 132 tests Rust passent après fusion. Le benchmark 4A du provisoire à douze wyvernes sur Ryzen 7 7730U donne 177 FPS moyens et 107,6 FPS en 1 % low à 720p. [Protocole, tests multijoueurs et limites](JALON_4A.md)

La commande `wyvern` est intégrée après validation du modèle artistique en multijoueur, des objets Git LFS et de l’import Bevy après fusion. Le modèle provisoire `assets/creatures/wyvern.json` reste sélectionné par défaut. Pour utiliser la version artistique, passer `--creature-manifest creatures/wyvern_art.json` au client Bevy. Elle comprend 23 articulations, six clips et trois palettes. [Rapport de production, empreintes et provenance](../art_pipeline/reports/wyvern.md)

Les appuis suivent une grille fixe calculée à partir du relief généré. Les modifications de terrain ne déplacent pas ces appuis. La persistance des créatures reste hors périmètre de 4A. Au redémarrage, le serveur reconstruit le groupe initial au tick zéro.

## Jalon 3D

Le client Bevy importe une wyverne provisoire, ses 23 articulations et six clips glTF. **Inspecter-Wyverne.cmd** lance la démonstration locale. Le serveur et le mode en réseau conservent le périmètre 3C. [Inspection, vidéo et résultats 0/1/4/12](CREATURES_3D.md) · [Contrat du modèle de remplacement](WYVERN_ASSET.md).

## Jalon 3C

Le preset optionnel **forest-modern** est jouable avec le serveur autoritaire, le client Bevy et le client de référence Macroquad. Les 111 tests Rust passent. Trois passages du binaire livré donnent une médiane de 161 FPS à 720p, 108 FPS en 1 % low, et un pic maximal de 853 Mio. Le paquet Windows ouvre une partie persistante avec **Jouer-Foret.cmd**. La visite en caméra libre reproduit les cadrages du benchmark.

[Guide de jeu et réglages](FORET_3C.md) · [Mesures, captures et validation multijoueur](PERFORMANCE_3C.md)

## Jalon 3B

Deux clients graphiques partagent le serveur autoritaire version 1. Les 91 tests Rust passent. Les essais sur fenêtres Windows vérifient les quatre éditions croisées, les deux reconnexions, le redémarrage du serveur et les deux avatars. Le ZIP comprend les trois exécutables. À 720p, sur le trajet réseau mobile, les médianes sont de 293 FPS pour Bevy et 596 FPS pour Macroquad. Le pic observé pour Bevy est de 549 Mio.

[Multijoueur et interfaces du prochain jalon](MULTIJOUEUR_3.md) · [Benchmarks Ryzen](PERFORMANCE_3.md)

## Jalon 3A — renderer Bevy validé en solo

Le client Bevy 0.20.1 et le client Macroquad rendent les mêmes 11 249 quads en 720p. Le contrôle de la caméra et les parcours lointains ont été exercés sur la Radeon. Les 90 tests du workspace en release passent. [Mesures et limites 3A](RENDU_3A.md)

## Historique du jalon 2

Le serveur Rust dédié, le client graphique Macroquad et le ZIP portable sont disponibles pour Windows x64. Des processus clients distincts ont validé les éditions concurrentes, les zones à 4,096 km, la reconnexion et la relecture des positions et du terrain après redémarrage. [Mode d’emploi et architecture réseau](MULTIJOUEUR_2.md) · [Mesures et limites](PERFORMANCE_2.md)

La prochaine tâche est de reproduire la mesure serveur sur le bi-Xeon, puis de tester la réplication et la persistance d’une première créature autoritaire.

## Historique du jalon 1C

Le client Windows utilise le streaming prédictif avec Macroquad et le greedy meshing. Les sauvegardes régionales et l’import v1 conservent leurs formats. Les mesures comparatives sur Ryzen 7 7730U, les bornes et les cas où le sol apparaît encore en retard figurent dans le [rapport 1C](STREAMING_1C.md).

Les 65 tests Rust passent, dont les 61 du jalon 1B. Sur la traversée de 4,22 km au rayon 2, la médiane passe de 244 à 4 images sans sol graphique sur 720. Les 60 passages comprennent trois répétitions pour chaque variante et chaque cas.

## Historique du jalon 1B

Le client Windows x64 permet de quitter à pied ou en vol la vallée historique. Les chunks chargés, leur maillage et les sauvegardes suivent le joueur. Les commandes, les collisions et les sept matériaux de la première version sont conservés. Le client utilise Macroquad. Le monde et les workers sont utilisables sans fenêtre.

| Sous-jalon | Résultat vérifié | Preuve |
|---|---|---|
| 1B.1 | Cent éditions comparables ont isolé la barrière disque. En 720p le P95 de l’édition en frontière sur le fil graphique passe de 22,34 à 0,67 ms sur la paire avant/après. | [Mesures](PERFORMANCE.md) |
| 1B.2 | Coordonnées étendues, génération à la demande, révisions, sauvegardes de région à CRC. La migration garde le fichier v1 intact. Sept empreintes de vallées complètes correspondent au générateur historique compilé depuis 1776a32. | [Architecture](ARCHITECTURE.md) · [Empreintes](../benchmarks/results/1b/empreintes-v1.txt) |
| 1B.3 | Le pool borne les travaux en vol, annule les calculs obsolètes et épingle les modifications jusqu’à l’accusé de sauvegarde. Le test Windows modifie une vallée importée, ferme et rouvre le client. | [Tests de concurrence](../crates/streaming/tests/concurrent.rs) · [Test Windows](../tools/test-window.py) |
| 1B.4 | Les parcours graphiques 512, 1 024, 4,22 km, téléportation et rafales de 64 blocs atteignent leurs points de contrôle. Le test sans fenêtre relit l’édition distante après passage par −1 000 000 et réouverture du stockage. | [Parcours](../benchmarks/results/1b/parcours/) · [Traversée sans fenêtre](../crates/streaming/tests/travel.rs) |
| 1B.5 | Greedy scalaire, greedy binaire, allocations réutilisées et culling ont été mesurés séparément. Les modes faces et greedy restent accessibles. | [Comparaison](PERFORMANCE.md) · [Audit Horizon et licences](HORIZON_AUDIT.md) |

## Contrôles

Le formatage, la compilation du workspace, Clippy sur toutes les cibles avec avertissements interdits, la construction release et les 61 tests Rust ont réussi. Les 34 tests historiques sont inclus. Les scénarios de corruption et d’arrêt brutal utilisent aussi des sous-processus. Le client Windows est testé par des entrées de fenêtre, avec relecture indépendante de la sauvegarde. Les mesures sont propres au Ryzen 7 7730U avec Radeon intégrée. Les budgets de streaming et le pic mémoire du processus sont deux mesures distinctes.
