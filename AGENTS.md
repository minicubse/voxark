# Travailler sur Primal Voxel

Lire d'abord ce fichier et `docs/PROJECT_STATUS.md`, puis examiner `git status --short` et `git log -5 --oneline`. Reprendre la prochaine action indiquée dans le statut. Conserver le travail existant et réserver des fichiers distincts aux contributeurs simultanés.

Construire chaque tranche dans le même workspace. Le noyau voxel et la simulation doivent rester utilisables sans fenêtre, pilote graphique ou périphérique d'entrée. Valider les intentions de jeu dans la simulation. Le futur serveur appellera cette même API.

Garder stables les identifiants de blocs. Toute modification du générateur doit conserver la lecture des sauvegardes antérieures, ou fournir une migration vérifiée. Ajouter un test des cas limites avant de modifier collisions, éditions, persistance ou synchronisation.

Depuis la racine, exécuter `cargo fmt --check`, `cargo check --locked --workspace`, `cargo clippy --locked --workspace --all-targets -- -D warnings` et `cargo test --locked --workspace`. Vérifier aussi le client en mode graphique après un changement de rendu ou d'entrée. Mesurer en release avec seed, résolution, matériel et scénario consignés.

Mettre à jour statut, limites et mesures après validation. Ne promettre un gain qu'après comparaison reproductible. Les crédits d'API, les pushes distants et les ressources propriétaires ne font pas partie du développement local.

## Production sur deux PC

À chaque début de jalon, PC 1 examine les nouveaux besoins en modèles, rigs, matériaux, textures et animations. Lire `art_pipeline/QUEUE.json`, examiner aussi les branches `art/*` en cours et mettre à jour ou créer les fiches dans `art_pipeline/specs/`. Ajouter les commandes avec leur identifiant stable sur `feat/code-*`, en préservant les commandes existantes et leur progression. Une fois cette PR intégrée et publiée sur `main`, PC 2 peut découvrir le travail par Git.

PC 1 possède les crates Rust, le serveur, les clients, le réseau, les outils partagés, les règles Git, les specs et l’intégration finale. Garder un modèle provisoire fonctionnel tant que la livraison artistique attend sa validation. Relier chaque modèle par manifeste. Réserver la production artistique à PC 2, sauf nécessité technique documentée.

PC 2 reprend ses commandes sur `art/<id>` suivant `art_pipeline/AGENT_ART_INSTRUCTIONS.md`. Seuls ses sources Blender, son export et son manifeste artistiques, ses prévisualisations, son rapport et les champs `status`/`notes` de sa commande sont de son ressort. PC 1 relit et teste chaque livraison selon `art_pipeline/INTEGRATION_PC1.md`, puis décide du statut `INTEGRATED` et de l’activation. Les pushes sur le remote partagé requièrent une destination réelle autorisée. Utiliser `art_pipeline/GIT_SETUP.md` pour le déploiement et les droits.
