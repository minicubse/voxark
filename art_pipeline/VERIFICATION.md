# Réception locale du jalon INFRA-GIT

## Dépôt et transmission

Au départ, `main` pointait sur `7bba57a`, avec 795 fichiers suivis et un arbre propre. Aucun remote, workflow GitHub, chemin de hooks personnalisé ni hook actif. Les règles de texte et d’images existantes ont été conservées. Le GLB précédemment présent dans l’historique reste lisible à sa révision initiale. La révision INFRA.1 stocke son pointeur LFS et livre exactement les mêmes octets dans le clone. Le manifeste provisoire conserve l’objet Git `6c0efb9444427c03a0e1305bc5dadad2f424df54`. Les branches historiques `graphics1b`, `storage1b`, `streamer1b` et `voxel1b` sont conservées.

| Essai | Résultat observé |
|---|---|
| Git LFS 3.5.1 | Installation locale, filtres actifs, `track`, `ls-files` et `fsck` vérifiés. GLB, .blend et textures dédiées suivent LFS. PNG documentaire et preview ordinaire restent dans Git. |
| Hooks | Quatre hooks Git for Windows actifs par `core.hooksPath`. Contrôle des chemins ignorés, sources suivables, détection du hook personnalisé et conservation du fichier. |
| Deux clones indépendants | Push Git + LFS réel vers un bare temporaire par URL `file://`. Clone sans smudge, lecture du pointeur, puis récupération du contenu et contrôle SHA-256 sur les deux clones. |
| Livraison PC 2 | PC 1 ajoute une commande temporaire, PC 2 la découvre et publie deux commits de progression. `main` garde `TODO` jusqu’à intégration. PC 1 télécharge le **nouvel objet LFS**, vérifie le SHA-256, valide le manifeste séparé, le sélectionne dans le vrai client Bevy puis intègre l’ordre. |
| Refus des écarts | 19 tests Git sur les chemins, règles imbriquées, modes spéciaux, renommages, changements de file et transitions. Une tentative réelle de push modifiant `AGENTS.md` est refusée et le head distant reste identique. |
| GitHub | Workflow relu, YAML analysé, action de checkout épinglée à son commit. Remote partagé absent. Exécution hébergée, droits réels, quotas, protections et téléchargement par le compte PC 2 restent à vérifier après publication. |

SHA-256 du GLB de référence : `7b3fe3e3c8bb4fd4a50d632c7d6eea66239f4e32c177290c6db4984dffcc95f4`. SHA-256 de la livraison de test : `fa5062efe23346c97857850780510adaa6b2b46394e3f433e2be8dc402d84bc3`. La livraison temporaire modifie seulement les métadonnées du GLB existant. Le dépôt de production conserve sa commande `wyvern` à `TODO`. [Résultat inter-clones](../benchmarks/results/infra-git/handoff.json)

## Outils et clients

- **Validation générique** — 20 tests CLI positifs et négatifs, dont le GLB livré, un glTF à ressources externes et noms personnalisés, les chemins invalides, les clips, les slots, les budgets, les poids, les indices des joints, les matrices de bind et la hiérarchie. Le validateur énumère les contrôles visuels restant à réaliser. [Rapport statique](../benchmarks/results/infra-git/static.json)
- **Client Bevy précompilé** — Import du provisoire sur Radeon/Vulkan avec 23 articulations, six clips et quatre slots. Huit scénarios de régression incluent le glTF à buffer externe et les échecs attendus. L’interface générique renvoie aussi un JSON d’échec pour un manifeste absent. [Import réel](../benchmarks/results/infra-git/native.json) · [Suite historique](../benchmarks/results/infra-git/regression.json)
- **Blender 4.5.0 headless** — Deux exports d’une scène de test temporaire ont donné le même SHA-256 `edb74cc648d22fc565911889d81d56f12028f49979e0541ddb3b696324f8c063`. Squelette, quatre matériaux, six actions et PNG incorporé. Le repère asymétrique a suivi `(2,-3,1) → (2,1,3) → (2,-3,1)`. Une source absente renvoie une erreur et garde le dernier GLB intact.
- **Sélection des commandes** — Le schéma versionné accepte la commande réelle. Les extraits PowerShell ont été analysés. Quatre scénarios Git vérifient la priorité numérique, l’exclusion de READY distant, et la reprise locale ou distante d’un travail IN_PROGRESS tandis que main indique encore TODO.
- **Moteur** — Format, compilation, Clippy avec avertissements interdits et 119 tests Rust réussis. En jeu, le serveur et les deux clients graphiques ont échangé quatre éditions, vérifié les deux reconnexions, le redémarrage et les avatars visibles. [Essai multijoueur](../benchmarks/results/infra-git/multiplayer.json)

## Mesures PC 1

Wyverne **provisoire**, Ryzen 7 7730U, Radeon intégrée, Vulkan, forêt Low, 1280 × 720, rayon 4, ombres des acteurs, cadrage fixe. Trois passages par effectif, 1 100 images chronométrées par passage. Médianes des trois passages. CPU exprimé en cœurs moyens, RAM en pic du working set par passage. La durée d’animation encadre l’exécution du groupe Bevy et inclut son ordonnancement.

| Wyvernes | FPS | 1 % low | P99 ms | CPU | RAM Mio | Triangles des créatures | Animation µs |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 0 | 173,4 | 92,3 | 9,79 | 2,44 | 863 | 0 | 286 |
| 1 | 148,5 | 73,5 | 12,00 | 2,52 | 619 | 6 048 | 454 |
| 4 | 152,9 | 81,3 | 11,18 | 2,52 | 867 | 24 192 | 469 |
| 12 | 136,5 | 71,0 | 12,12 | 2,46 | 602 | 72 576 | 486 |

Ces douze passages établissent une référence d’intégration. Refaire la comparaison avec le véritable asset artistique, sur le même poste et dans la même session, avant de statuer sur le seuil de 60 FPS. [Synthèse et références des passages](../benchmarks/results/infra-git/summary.json)

## Reproduire

Depuis la racine du clone équipé de Git LFS (Python standard pour les tests de validation). Le test de transmission crée puis nettoie ses dépôts dans le dossier temporaire. L’exécutable de validation doit se trouver hors Git.

~~~powershell
python tools/test-art-branch.py
python -m unittest discover -s tools/tests -p test_validate_creature.py
python tools/validate-creature.py --manifest assets/creatures/wyvern.json
$client = (Resolve-Path (Read-Host 'Chemin de PrimalVoxel-Bevy.exe')).Path
python tools/test-creature-import.py --client $client --manifest assets/creatures/wyvern.json --asset-root assets
python tools/test-art-handoff.py --client $client
~~~

Les commandes de publication et d’accès figurent dans [le guide Git](GIT_SETUP.md). Les contrôles locaux ont été exécutés sous Windows. Le workflow Ubuntu pourra être exécuté sur la plateforme choisie après publication.
