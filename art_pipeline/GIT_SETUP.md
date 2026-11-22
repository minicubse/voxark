# Dépôt commun et mise en service

`main` est la version stable. PC 1 développe sur `feat/code-*`, PC 2 sur une branche `art/<id>` par commande. L’historique du moteur et les fichiers artistiques appartiennent au même dépôt. Une Release sert à distribuer un outil exécutable, la file et les sources se synchronisent par Git.

## Premier déploiement par le propriétaire

Le dépôt d’origine du jalon INFRA-GIT n’avait aucun remote. Choisir ou créer le vrai dépôt partagé et donner aux deux PC accès aux objets Git **et** LFS. Depuis le clone PC 1, vérifier la destination avant publication. Ces commandes ciblent un nouveau distant vide. Si `git ls-remote` affiche des références existantes, examiner l’historique distant avec `git fetch origin` et rapprocher les branches par PR.

~~~powershell
git remote -v
$url = Read-Host 'URL réelle du dépôt partagé'
git ls-remote $url
if ($LASTEXITCODE) { throw 'Distant inaccessible' }
git remote add origin $url
git push -u origin main feat/code-art-pipeline
git ls-remote --heads origin main feat/code-art-pipeline
~~~

Si un `origin` a été configuré depuis, vérifier `git remote get-url origin` et réutiliser le remote correspondant. Sur un dépôt préexistant protégé, pousser la branche d’infrastructure, proposer sa PR, puis vérifier la présence de son commit sur `origin/main`. Conserver les permissions et historiques existants. Configurer `main` comme branche par défaut.

## Contrôles GitHub à réaliser après publication

Le propriétaire ou un administrateur ouvre **Settings → Rules → Rulesets** ou **Settings → Branches**. Cibler `main`, imposer une PR, une approbation indépendante et le contrôle artistique du workflow `art-branch-guard.yml`, interdire les force-pushes et les suppressions, limiter les contournements. Vérifier le nom exact du contrôle après sa première exécution et tester une PR rejetée. Exiger que les branches soient à jour avant fusion. Sur chaque nouveau jalon de code, PC 1 reste responsable de la compilation et des tests. Examiner les modifications à `.github/` avec les droits de PC 1.

La disponibilité dépend de la visibilité et du plan. GitHub Free inclut les protections pour les dépôts publics. Pour un dépôt privé, vérifier le plan, la disponibilité du contrôle et les droits d’administration. Un workflow contrôle une PR. Il faut également restreindre les écritures directes. Pour que PC 2 dispose d’une isolation même avec des hooks locaux désactivés, préférer un compte sans droit d’écriture sur le dépôt principal et des PR depuis son fork lorsque la politique LFS/les permissions du dépôt le permettent. Vérifier le téléchargement LFS depuis un second compte. Faute de protection disponible, réserver l’écriture du dépôt partagé à PC 1 et faire examiner chaque livraison par PR. [Documentation GitHub des branches protégées](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-protected-branches/about-protected-branches)

Les versions successives d’un gros binaire consomment chacune leur taille complète dans LFS. Les téléchargements utilisent la bande passante du propriétaire. La documentation indique **10 Gio de stockage et 10 Gio de bande passante incluse** pour GitHub Free. Contrôler les limites, l’utilisation et le budget du compte avant les gros exports. [Facturation officielle Git LFS](https://docs.github.com/en/billing/concepts/product-billing/git-lfs)

## Distribuer le validateur Bevy

Le paquet Windows x64 du jalon 3D contient le client compatible avec le manifeste v1, ses licences et les lanceurs. PC 1 peut joindre **PrimalVoxel-3D-Windows-x64.zip** à une Release du dépôt partagé après publication, puis communiquer l’URL de cette Release et les empreintes. SHA-256 du ZIP de référence : `f8e42d08ec09f55c9534c60eeffb68c96e0d4bf50d7ee6202d121499470f6a83`. SHA-256 de `PrimalVoxel-Bevy.exe` extrait : `375612d187f8fa516ace9f3f850a960bef6b800db5de1f9ca8a904795ef71472`. Pour un nouveau binaire, publier les sommes correspondantes et la révision des sources après un essai d’import. PC 2 installe le ZIP hors du clone et vérifie le fichier avec `Get-FileHash -Algorithm SHA256`.

## Activer un clone

Exécuter depuis la racine du nouveau clone. Windows utilise Git for Windows, Git LFS et Python. Linux utilise Git, Git LFS, Python 3 et un shell POSIX pour les hooks.

~~~powershell
git lfs version
python tools/install-art-hooks.py
git config --get core.hooksPath
git lfs pull
git lfs ls-files
git lfs fsck
~~~

Le programme compare les hooks existants avec ceux fournis par la version installée de Git LFS et signale les hooks personnalisés à rapprocher. Il configure les filtres localement puis applique `git config core.hooksPath .githooks`. Les hooks versionnés relaient les événements LFS. `pre-push` garde les références et l’entrée standard destinées à LFS après contrôle. Le hook local est un filet de sécurité. Le workflow de PR utilise le validateur provenant de la base.

Pour le contrôle manuel d’une PR depuis un clone de confiance :

~~~powershell
git fetch origin
python tools/check-art-branch.py --base origin/main --head origin/art/wyvern --branch art/wyvern
~~~

**Feu vert PC 2** uniquement après avoir vérifié depuis un second clone du vrai remote que `origin/main` contient le commit d’infrastructure, que la branche d’infrastructure est disponible, que Git LFS a livré le GLB complet et que le client de validation est accessible. Ensuite suivre [le manuel PC 2](AGENT_ART_INSTRUCTIONS.md).
