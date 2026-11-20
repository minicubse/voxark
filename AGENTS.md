# Travailler sur Primal Voxel

Lire d'abord ce fichier et `docs/PROJECT_STATUS.md`, puis examiner `git status --short` et `git log -5 --oneline`. Reprendre la prochaine action indiquée dans le statut. Conserver le travail existant et réserver des fichiers distincts aux contributeurs simultanés.

Construire chaque tranche dans le même workspace. Le noyau voxel et la simulation doivent rester utilisables sans fenêtre, pilote graphique ou périphérique d'entrée. Valider les intentions de jeu dans la simulation. Le futur serveur appellera cette même API.

Garder stables les identifiants de blocs. Toute modification du générateur doit conserver la lecture des sauvegardes antérieures, ou fournir une migration vérifiée. Ajouter un test des cas limites avant de modifier collisions, éditions, persistance ou synchronisation.

Depuis la racine, exécuter `cargo fmt --check`, `cargo check --locked --workspace`, `cargo clippy --locked --workspace --all-targets -- -D warnings` et `cargo test --locked --workspace`. Vérifier aussi le client en mode graphique après un changement de rendu ou d'entrée. Mesurer en release avec seed, résolution, matériel et scénario consignés.

Mettre à jour statut, limites et mesures après validation. Ne promettre un gain qu'après comparaison reproductible. Les crédits d'API, les pushes distants et les ressources propriétaires ne font pas partie du développement local.
