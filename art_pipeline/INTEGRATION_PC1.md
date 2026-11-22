# Intégrer une livraison artistique

PC 1 garde la décision d’intégration et d’activation. Le manifeste par défaut reste `assets/creatures/wyvern.json`. La livraison attendue de PC 2 possède son propre manifeste `assets/creatures/wyvern_art.json` et son dossier `wyvern_art/`. Garder séparées la réception de l’asset et sa sélection dans le jeu.

## Examiner une livraison

Depuis un clone PC 1 propre et équipé de Git LFS et de Python, en PowerShell. Remplacer `wyvern` par l’identifiant de la commande. Faire l’examen avant d’exécuter tout code issu de la branche. En présence de modifications locales, utiliser un autre worktree.

~~~powershell
$id = 'wyvern'
git switch main
git status --short --branch
git fetch --prune origin
git pull --ff-only origin main
$base = git rev-parse HEAD
$head = git rev-parse "origin/art/$id"
git log --oneline "$base..$head"
git diff --stat "$base...$head"
python tools/check-art-branch.py --base $base --head $head --branch "art/$id"
if ($LASTEXITCODE) { throw "Périmètre artistique refusé" }
git lfs fetch origin $head
$review = "../review-$id"
git worktree add --detach $review $head
Push-Location $review
git lfs checkout
git lfs fsck
Get-Content "art_pipeline/reports/$id.md"
Get-ChildItem "art_pipeline/previews/$id"
~~~

Confirmer la provenance des sources, des textures et de leurs licences dans le rapport. Examiner les vues face, profil, dos, trois quarts et les poses. Vérifier silhouette et proportions, pli des membranes, absence d’étirement anormal et absence de déplacement imposé par le clip. Comparer le rendu des membranes des deux côtés avec et sans ombres. L’ouverture d’un .blend ou l’exécution d’un script de livraison demandent une revue distincte. Le contrôle du périmètre lit des objets Git et ne lance aucun script proposé.

## Valider et mesurer

Depuis le worktree d’examen. Le client précompilé compatible est celui de la distribution Windows 3D, nommé `PrimalVoxel-Bevy.exe`. Pointer la variable vers le fichier extrait. Le benchmark PC 1 utilise Python 3.11+ et `psutil`. L’import seul fonctionne sans Rust.

~~~powershell
$client = (Resolve-Path (Read-Host 'Chemin de PrimalVoxel-Bevy.exe')).Path
$manifest = "assets/creatures/${id}_art.json"
python tools/validate-creature.py --manifest $manifest --asset-root assets --result benchmarks/tmp/asset.json
if ($LASTEXITCODE) { throw "Asset rejeté" }
python tools/test-creature-import.py --client $client --manifest $manifest --asset-root assets --result benchmarks/tmp/import.json
if ($LASTEXITCODE) { throw "Import Bevy rejeté" }
& $client --asset-root (Resolve-Path assets).Path --preset forest-modern --creatures 4 --creature-state demo --creature-manifest "creatures/${id}_art.json"
~~~

Inspecter les six clips (touches 1 à 6), leurs transitions (0) et la phase de vol au retour de caméra. Contrôler les trois palettes Jade, Braise et Obsidienne sur quatre acteurs. Comparer avec le même client et la même scène en activant le provisoire.

~~~powershell
$common = @('--client', $client, '--asset-root', 'assets', '--repeats', '3', '--cases', '0', '1-near-shadow-on', '4-near-shadow-on', '12-near-shadow-on')
foreach ($model in @('wyvern', "${id}_art")) {
    $out = "benchmarks/tmp/accept-$model"
    python tools/benchmark-creatures.py measure @common --creature-manifest "creatures/$model.json" --output $out
    if ($LASTEXITCODE) { throw "Mesure incomplète pour $model" }
    python tools/benchmark-creatures.py summarize --output $out
    if ($LASTEXITCODE) { throw "Résumé incomplet pour $model" }
}
cargo fmt --check
cargo check --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
~~~

Utiliser le Ryzen 7 7730U, Radeon matérielle, Vulkan, Low forêt, 1280 × 720, seed 20261120, rayon 4, cadrage fixe et ombres actives. Garder le GPU disponible. Archiver les mesures par scénario (moyenne FPS, 1 % low, P99, CPU, pic RAM, triangles et coût des animations), l’identité des binaires et les empreintes des deux modèles. Seuil d’acceptation minimal pour cette scène jusqu’à douze acteurs proches : 60 FPS moyens à chaque effectif. Examiner aussi les saccades et les écarts au provisoire avec l’auteur. Si le seuil ou la qualité visuelle échoue, garder le provisoire et demander une optimisation ou des LOD réellement exportés. Pour une reprise après READY, PC 1 consigne la décision et convient avec PC 2 des corrections avant une nouvelle proposition.

## Fusion puis activation

1. Attendre un rapport complet, toutes les vérifications vertes et une file cohérente. Relire le diff depuis le dernier `main` si la cible a bougé. Garder tout nouvel ordre ajouté entre-temps.
2. Faire fusionner **la révision examinée** de la PR artistique vers `main` en conservant les commits de progression. Le statut livré est `READY_FOR_INTEGRATION`. La seule mise à jour de la branche artistique n’avait pas changé `main`.
3. Revenir au clone PC 1 (`Pop-Location`), synchroniser `main` et créer `git switch -c "feat/code-integrate-$id"`. Dans la file, passer uniquement la commande acceptée à `INTEGRATED`. Référencer le rapport et les mesures dans la PR de code. En cas de conflit JSON, rapprocher les entrées par identifiant et préserver les champs de définition de PC 1 ainsi que les notes validées de PC 2.
4. L’activation individuelle est déjà possible avec `--creature-manifest`. Pour changer le défaut du jeu, modifier la sélection sur la branche de code, refaire les tests puis fusionner la PR. Le serveur et les sauvegardes n’ont besoin d’aucune modification. Conserver le manifeste et le GLB provisoires comme référence de repli.
