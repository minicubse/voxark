# Feuille de route

Les états constatés et les preuves exécutées sont dans PROJECT_STATUS.md. Les jalons ci-dessous fixent les critères d'acceptation, sans date de livraison estimée.

| Jalon | Résultat à livrer | Critère de validation |
| --- | --- | --- |
| 0 — Fondation | Workspace Rust, noyau autonome, chaîne de compilation et Git | Reproduction des contrôles fmt, check, clippy et tests |
| 1a — Monde initial | Terrain, chunks, meshing, caméra, collisions, édition et sauvegarde | Construire/détruire puis retrouver le terrain après redémarrage |
| 1b — Monde extensible | Données matériaux validées, streaming asynchrone, budgets de jobs/uploads | Traversée rapide sans blocage prolongé, mémoire bornée mesurée |
| 1c — Comparaison | Tailles 16³/32³/64³, mesher baseline/greedy, essai frontend Bevy | Scène et trajet identiques, mesures conservées, choix justifié |
| 2 — Réseau | Serveur headless autoritaire, deux clients, reconnexion | Édition partagée, convergence après reconnexion, persistance serveur |
| 3 — Première créature | Espèce originale animée, IA, combat, torpeur, apprivoisement, monte | Apprivoiser, monter, redémarrer le serveur, retrouver la créature |
| 4 — Survie | Besoins, récolte, outils, inventaire, craft, déblocages, base, mort/respawn | Boucle de survie jouable, transferts sans duplication |
| 5 — Écosystème | Espèces, biomes, navigation, reproduction et génétique | Écosystème persistant et règles d'héritage testées statistiquement |
| 6 — Progression | Grottes, équipements, ressources, capacités et premier boss | Progression PvE complète avec récompense persistante |
| 7 — Extension Primal | Catégories originales, nourritures, pouvoirs, équipements et boss | Une route avancée complète avec options tactiques |
| 8 — Modding | Formats externes versionnés, validation, dépendances, capacités | Nouvelle créature et nouvelle capacité ajoutées par données |
| 9 — Couverture | Détail progressif des deux matrices, exceptions par mécanique | Chaque ligne clôturée possède source, implémentation et preuve |
| 10 — Finition | Stress tests, profils Low, interface, animation, son et distribution | Objectifs CPU/GPU/mémoire vérifiés sur matériel cible |

Après 1B, réduire les frames où le sol arrive en retard sur le trajet de 4,22 km. Comparer les politiques de préchargement à contenu, vitesse et budgets identiques. Reporter l’essai de backend au moment où une mesure isolera un coût du rendu susceptible de le justifier.
