# Produire un asset depuis PC 2

Travailler dans le clone du dépôt Git partagé, une fois le pipeline disponible sur `origin/main`. PC 1 définit les commandes et les spécifications. PC 2 produit les fichiers artistiques sur `art/<id>`. Les commandes PowerShell ci-dessous s’exécutent depuis la racine du clone. Préparer Git, Git LFS, Python 3.10 ou ultérieur, Blender 4.5 LTS et le client Bevy précompilé fourni par PC 1.

## 1. Récupérer le dépôt et reprendre un travail existant

```powershell
git --version
git lfs version
python --version
git remote -v
git status --short --branch
git fetch --prune origin
if ($LASTEXITCODE -ne 0) { throw 'Récupération du dépôt distant en échec' }
git log -1 --oneline origin/main
```

Si le clone est à créer, utiliser `git clone` avec l’URL du dépôt fournie par PC 1. En présence de modifications locales, examiner leur contenu. Continuer sur leur branche artistique si elles lui appartiennent. Sauvegarder les modifications dans leur branche d’origine avant de changer de branche.

Le carnet [QUEUE.json](QUEUE.json) est un objet versionné dont `orders` contient les commandes décrites dans [QUEUE.schema.json](QUEUE.schema.json). Chaque `id` est unique et détermine `art/<id>`, `art_pipeline/specs/<id>.md`, `assets/creatures/<id>_art.json` et `assets/creatures/<id>_art/`. `priority` commence à `1`, qui est la priorité la plus élevée. `replaces` indique la référence provisoire et `assigned_to` désigne le responsable. PC 2 peut modifier uniquement `status` et `notes` de sa commande sur sa branche.

La sélection commence par les travaux `IN_PROGRESS` attribués à `PC2-ZCode`, y compris ceux dont `origin/main` affiche encore `TODO`. Sans commande en cours, sélectionner le `TODO` de plus petite priorité numérique. À priorité égale, prendre le premier identifiant par ordre alphabétique. Une commande déjà `READY_FOR_INTEGRATION` ou `INTEGRATED` sort de la sélection automatique, y compris si le statut apparaît seulement sur une branche artistique. Si la commande vient de changer dans le dépôt distant après la récupération, relire son statut avant toute nouvelle prise en charge.

```powershell
$queue = git show 'origin/main:art_pipeline/QUEUE.json' | ConvertFrom-Json
if ($queue.schema_version -ne 1) { throw 'Version de QUEUE.json inconnue' }

$eligible = foreach ($entry in ($queue.orders | Sort-Object priority, id)) {
    if ($entry.assigned_to -ne 'PC2-ZCode' -or $entry.status -notin @('TODO', 'IN_PROGRESS')) {
        continue
    }
    $name = "art/$($entry.id)"
    $statuses = @($entry.status)
    foreach ($ref in @("refs/heads/$name", "refs/remotes/origin/$name")) {
        git show-ref --verify --quiet $ref
        if ($LASTEXITCODE -eq 0) {
            $branchQueue = git show "${ref}:art_pipeline/QUEUE.json" | ConvertFrom-Json
            $branchOrder = $branchQueue.orders | Where-Object { $_.id -eq $entry.id }
            if ($null -eq $branchOrder) { throw "Commande introuvable sur $ref" }
            $statuses += $branchOrder.status
        }
    }
    if ('READY_FOR_INTEGRATION' -in $statuses -or 'INTEGRATED' -in $statuses) { continue }
    [pscustomobject]@{ Order = $entry; InProgress = 'IN_PROGRESS' -in $statuses }
}

$selection = $eligible | Where-Object InProgress | Select-Object -First 1
if ($null -eq $selection) { $selection = $eligible | Select-Object -First 1 }
if ($null -eq $selection) { throw 'Aucune commande à traiter' }
$order = $selection.Order
$id = $order.id
$branch = "art/$id"
git show "origin/main:$($order.spec_path)"
```

Lire la fiche affichée. Si la branche locale existe, basculer dessus. Si seule la branche distante existe, créer sa branche locale de suivi. Pour une nouvelle commande, créer la branche depuis `origin/main`.

```powershell
git show-ref --verify --quiet "refs/heads/$branch"
if ($LASTEXITCODE -eq 0) {
    git switch $branch
} else {
    git show-ref --verify --quiet "refs/remotes/origin/$branch"
    if ($LASTEXITCODE -eq 0) {
        git switch --track -c $branch "origin/$branch"
    } else {
        git switch -c $branch origin/main
    }
}
if ($LASTEXITCODE -ne 0) { throw 'Changement de branche en échec' }
git status --short --branch
python tools/install-art-hooks.py
if ($LASTEXITCODE -ne 0) { throw 'Installation des hooks Git en échec' }
git lfs pull
if ($LASTEXITCODE -ne 0) { throw 'Récupération des fichiers LFS en échec' }
git lfs status
```

L’installateur préserve les hooks Git LFS existants et signale les hooks personnalisés. Si l’installation est bloquée, suivre le [guide Git](GIT_SETUP.md).

Sur une branche déjà suivie, après sauvegarde du travail local, récupérer les commits distants avec `git merge --ff-only "origin/$branch"`. Si les deux historiques ont avancé, les examiner avec `git log --left-right --oneline "HEAD...origin/$branch"` et résoudre la divergence par fusion en conservant les deux travaux. Quand PC 1 ajoute une commande ou met à jour une fiche en parallèle, `git fetch origin main` puis `git merge origin/main` permet de récupérer ces changements. En cas de conflit dans `QUEUE.json`, conserver les entrées et les champs définis par PC 1 ainsi que les `status` et `notes` de sa propre commande. Examiner le résultat avant de terminer la fusion. Éviter de remplacer la file entière par la version de l’une des branches.

## 2. Réserver la commande sur la branche artistique

Si l’entrée locale est `TODO`, modifier uniquement son `status` en `IN_PROGRESS` et, au besoin, ses `notes`. Pousser cette réservation dans un commit distinct de la livraison. Elle permet de reprendre le travail après interruption même lorsque `main` affiche toujours `TODO`.

```powershell
git commit -m "Démarrer la commande $id" -- art_pipeline/QUEUE.json
if ($LASTEXITCODE -ne 0) { throw 'Commit de réservation en échec' }
git fetch origin main
if ($LASTEXITCODE -ne 0) { throw 'Récupération de main en échec' }
python tools/check-art-branch.py --base origin/main --head HEAD --branch $branch
if ($LASTEXITCODE -ne 0) { throw 'Périmètre de branche invalide' }
git push --set-upstream origin $branch
```

Si la commande est déjà `IN_PROGRESS`, poursuivre le travail sur cette branche. Si la branche est déjà `READY_FOR_INTEGRATION` ou `INTEGRATED`, laisser PC 1 traiter la livraison. `INTEGRATED` est renseigné par PC 1 sur une branche de code après validation et fusion. Une progression poussée sur `art/<id>` reste propre à cette branche jusqu’à la fusion.

## 3. Produire et exporter

Lire la fiche indiquée par `$order.spec_path` et créer le manifeste à `$order.manifest_path` au cours de la production. Conserver le travail de sculpture, de rig, de skinning et d’animation dans la source `.blend` finale, enregistrée avec la compression Blender. Conserver les textures PNG externes sous `textures/` avec chemins relatifs `//textures/...`. Pour Wyvern, réserver la scène `Wyvern` aux éléments exportables et conserver les aides de présentation dans une autre scène. Préparer les six Actions nommées et associées au rig de la scène selon [la fiche Wyvern](specs/wyvern.md). Réaliser et documenter un aller-retour glTF avec un repère asymétrique sur une copie de travail.

Pour Wyvern, le script [build.py](../tools/blender/wyvern/build.py) ouvre la source existante et exporte la scène. Pour toute autre commande livrée sans script, créer `tools/blender/<id>/build.py` d’après la fiche et le fichier source. Exécuter depuis la racine en remplaçant la valeur de `$blender` par le chemin du programme installé sur PC 2.

```powershell
$blender = 'C:\Program Files\Blender Foundation\Blender 4.5\blender.exe'
& $blender --version
& $blender --background --factory-startup --disable-autoexec --python-exit-code 1 --python "tools/blender/$id/build.py"
if ($LASTEXITCODE -ne 0) { throw "Échec de l’export Blender" }
Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $order.output_path "$id.glb")
```

Pour tester une copie, ajouter `-- --source CHEMIN.blend --output CHEMIN.glb` après le chemin du script. Employer la même version exacte de Blender pour vérifier la reproductibilité et l’indiquer dans le rapport. Le GLB est exporté en Y vertical et incorpore les PNG utilisés. Contrôler le rendu dans les vues de face, profil, dos et trois quarts, plusieurs poses d’animation et les trois palettes. Déposer les prévisualisations sélectionnées dans `art_pipeline/previews/<id>/`. Garder les rendus de travail temporaires hors des prévisualisations à versionner.

## 4. Valider avec le client précompilé et renseigner le rapport

Récupérer `PrimalVoxel-Bevy.exe` dans le paquet Windows `PrimalVoxel-3D-Windows-x64.zip` fourni par PC 1 sur une Release ou comme artefact du pipeline. Décompresser le paquet dans un dossier local extérieur au dépôt. Le SHA-256 de l’exécutable de validation v1 est `375612d187f8fa516ace9f3f850a960bef6b800db5de1f9ca8a904795ef71472`. En cas de version ultérieure, comparer la somme avec celle publiée par PC 1. Consigner la provenance, la version et la somme vérifiée dans le rapport. Si le binaire compatible est indisponible, conserver la commande à `IN_PROGRESS` et demander le paquet à PC 1. La validation statique reste exécutable seule.

```powershell
$client = 'C:\chemin\vers\PrimalVoxel-Bevy.exe'
Get-FileHash -Algorithm SHA256 -LiteralPath $client
$statique = Join-Path $env:TEMP "$id-validation-statique.json"
$importBevy = Join-Path $env:TEMP "$id-validation-bevy.json"
python tools/validate-creature.py --manifest $order.manifest_path --asset-root assets --result $statique
if ($LASTEXITCODE -ne 0) { throw 'Échec de la validation statique' }
python tools/test-creature-import.py --client $client --manifest $order.manifest_path --asset-root assets --result $importBevy
if ($LASTEXITCODE -ne 0) { throw "Échec de l’import Bevy" }
Get-Content -LiteralPath $statique, $importBevy
```

Inspecter aussi dans Bevy l’orientation, les appuis, les articulations, les six clips, les trois palettes et les deux faces des membranes. Renseigner `art_pipeline/reports/<id>.md` avec les résultats, les commandes et les liens vers les prévisualisations, en suivant [le modèle de rapport](reports/wyvern.md). Conserver les résultats temporaires en dehors du dépôt et reporter les valeurs utiles dans le Markdown.

## 5. Livrer la branche

Après validation, passer uniquement la commande sélectionnée de `IN_PROGRESS` à `READY_FOR_INTEGRATION`. Le périmètre de la branche comprend `art_pipeline/QUEUE.json`, `art_pipeline/reports/<id>.md`, `art_pipeline/previews/<id>/**`, `assets/creatures/<id>_art.json`, `assets/creatures/<id>_art/**` et `tools/blender/<id>/**`. Les manifestes provisoires, le moteur, les règles du dépôt, les fiches et les outils partagés relèvent de PC 1.

```powershell
git add -- art_pipeline/QUEUE.json "art_pipeline/reports/$id.md" "art_pipeline/previews/$id/" $order.manifest_path $order.output_path "tools/blender/$id/"
if ($LASTEXITCODE -ne 0) { throw 'Ajout des fichiers de livraison en échec' }
git diff --cached --name-status
git lfs status
git commit -m "Livrer la commande $id pour intégration"
if ($LASTEXITCODE -ne 0) { throw 'Commit de livraison en échec' }
git fetch origin main
if ($LASTEXITCODE -ne 0) { throw 'Récupération de main en échec' }
python tools/check-art-branch.py --base origin/main --head HEAD --branch $branch
if ($LASTEXITCODE -ne 0) { throw 'Périmètre de branche invalide' }
git lfs fsck
if ($LASTEXITCODE -ne 0) { throw 'Vérification Git LFS en échec' }
git push --set-upstream origin $branch
```

Ouvrir une Pull Request vers `main`, avec un lien vers le rapport et les limites connues. Sur GitHub, utiliser le formulaire ou `gh pr create --base main --head $branch --fill` si `gh` est disponible et authentifié. PC 1 examine la livraison, valide l’intégration et choisit si et quand activer l’asset par défaut.
