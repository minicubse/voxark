# Primal Voxel — état du jalon 1B

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

## Prochaine action

Le compteur de sol disponible pendant les vols rapides met en évidence des retards que les FPS moyens masquent. Mesurer la priorité du terrain sous la trajectoire future à nombre de workers et mémoire identiques. La position du joueur, le serveur réseau, les créatures et la survie restent des étapes ultérieures. [Limites](KNOWN_ISSUES.md) · [Feuille de route](ROADMAP.md)
