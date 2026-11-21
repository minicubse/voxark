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

## Premier serveur autoritaire

1. Exécuter la simulation à pas fixe dans un processus sans fenêtre. Fournir à chaque tick la liste des positions et vitesses validées. Prioriser collisions et sauvegardes sous le budget global. Permettre au serveur de désactiver la préparation des maillages graphiques.
2. Connecter deux clients. Le serveur attribue les identités, valide déplacements et interactions, puis envoie les instantanés de chunks suivis des changements de révision. Identifier les commandes par joueur et numéro de séquence pour appliquer une retransmission une seule fois.
3. Tester d’abord les deux joueurs dans la même région. Vérifier qu’une édition concurrente produit le même état autoritaire chez les deux clients. Tester ensuite deux régions éloignées, une téléportation puis une déconnexion. Au retour, demander un instantané en cas de révision manquante.
4. Critère de passage proposé : après acquittement, comparer blocs et révisions sur les deux clients et le serveur. Redémarrer le serveur, relire les éditions sur disque et rejouer un ancien message. Vérifier aussi la suspension du mouvement vers une zone inconnue et les plafonds de résidence sur les deux intérêts.

Le jalon 1C exerce déjà le noyau avec deux intérêts à 1,9 million de blocs de distance. Le jalon 2 a depuis validé cette convergence avec des processus distincts, y compris après fermeture et redémarrage. [Validation et prochaine tâche](MULTIJOUEUR_2.md)
