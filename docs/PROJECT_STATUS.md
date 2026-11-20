# État au 20 novembre 2026

**Jalons 0 et 1a validés sur Windows x64.** Le client solo ouvre une vallée procédurale de 128 × 64 × 128 blocs. Le joueur marche, saute, sprinte, vole avec collisions, détruit et construit avec sept matériaux. Les modifications sont enregistrées automatiquement et relues à la réouverture.

Le workspace comporte quatre crates. Le noyau et la simulation sont indépendants du rendu. Le client Macroquad sert de référence pour le futur essai Bevy. Le catalogue de matériaux est validé, le format PVXD v1 est versionné et le générateur v1 possède une empreinte de référence.

## Preuves

- `cargo fmt --all --check`, `cargo check --locked --workspace` et les deux commandes Clippy (workspace et tous targets avec `-D warnings`) passent.
- `cargo test --locked --workspace` : 34 tests réussis, dont 11 voxel, 12 simulation, 5 persistance et 6 client. La comparaison greedy couvre géométrie, orientation et matériau sur tout le monde avant et après édition.
- Un test de persistance démarre un second processus, l'arrête immédiatement après synchronisation du fichier de staging, puis vérifie le verrou et l'ancienne sauvegarde.
- `python tools/test-window.py <exe>` vérifie, par les événements de la fenêtre Windows, capture, sauvegarde F5, destruction, redémarrage, sélection de cristal et repose. Le résultat est contrôlé sur disque après les deux lancements.
- Huit parcours graphiques release, deux tailles de fenêtre, deux meshers et deux séries. Le mode greedy par défaut ramène la scène à 24 758 quads. [Mesures](PERFORMANCE.md) · [Capture du jeu](vallee.png)

## Prochaine tâche

**Jalon 1b.** Ajouter un parcours de traversée avec éditions en bord de chunk, chronométrer séparément reconstruction CPU, soumission au pilote et sauvegarde. Utiliser ces mesures pour dimensionner une file de génération et de remeshing asynchrones à budget par frame. Tester qu'une édition plus récente survit à l'arrivée d'un résultat de calcul périmé. La vallée actuelle fournit la référence de correction et de performance.

La réplication à deux clients, le serveur dédié, les créatures et la boucle de survie sont à développer aux jalons ultérieurs. [Feuille de route](ROADMAP.md) · [Limites](KNOWN_ISSUES.md)
