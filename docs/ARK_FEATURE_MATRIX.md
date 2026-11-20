# Matrice des mécaniques ARK

Établie le 20 novembre 2026. Référence fonctionnelle ARK Survival Evolved. La version exacte du jeu et les paramètres serveur devront accompagner toute future règle numérique. Les familles viennent de la mission utilisateur, la recherche initiale confirme leur cadre général. Les exceptions par espèce, carte et extension restent à vérifier avant implémentation.

La page officielle décrit survie, récolte, construction, craft, apprivoisement, montures, commandes et élevage avec héritage, incubation et gestation. Elle confirme aussi faim, soif, poids, température, structures liées et support. Les valeurs statistiques et probabilités ne sont pas établies par cette source. [Page éditeur](https://store.steampowered.com/app/346110/ARK_Survival_Evolved/).

Le statut « prévu » signifie mécanique absente du prototype. Le statut de validation du monde voxel et des commandes figure dans PROJECT_STATUS.md.

| Famille demandée | Mécaniques à couvrir | Systèmes concernés | État | Preuve exigée pour clôture |
| --- | --- | --- | --- | --- |
| Survie vitale | Santé, endurance, faim, soif, oxygène, poids | Simulation, survie, inventaire | Prévu | Pas fixe, bornes, consommation et sauvegarde vérifiées |
| Environnement et états | Température, fortitude, torpeur, maladies, blessures, résistances, buffs et debuffs | Survie, combat, biomes | Prévu | Interactions cumulées et expiration testées |
| Cycle joueur | Mort, respawn, corps récupérable, expérience, niveaux, statistiques | Survie, progression, persistance | Prévu | Mort/reconnexion sans perte ni duplication |
| Inventaire | Piles, conteneurs, poids, durabilité, qualités, raretés, transfert rapide | Inventaire, réseau | Prévu | Transferts atomiques, limites, déconnexion concurrente |
| Ressources | Récolte, outils spécialisés, rendements, renouvellement, rareté, péremption | Inventaire, monde, survie | Prévu | Coûts et récompenses autoritaires, temps persistants |
| Transformation | Stockage avancé, stations, raffinage | Craft, inventaire, construction | Prévu | Recettes et sorties cohérentes après interruption |
| Craft et équipement | Engrammes, blueprints, qualités, armures, armes, selles, munitions, explosifs | Craft, progression, combat | Prévu | Données validées et fabrication sans duplication |
| Technologie | Énergie, électronique, technologies avancées, déblocages spéciaux | Construction, progression | Prévu | Graphe de dépendances complet et alimentations testées |
| Édition voxel | Pose, destruction, sélection, remeshing, persistance | Voxel, simulation, client, stockage | Prototype initial | Édition visible, limites/reach validés, rechargement identique |
| Structures | Fondations, murs, plafonds, piliers, portes, portails, rampes, escaliers, fenêtres | Construction, physique | Prévu | Placement, support et collisions sur frontières de chunks |
| Défense et accès | Tourelles, pièges, permissions de tribu, protection | Construction, combat, réseau | Prévu | Requêtes refusées si propriétaire/permissions invalides |
| Usure et réparation | Durabilité, dégâts, dégradation configurable, réparation | Construction, inventaire | Prévu | Coûts et transitions sauvegardés atomiquement |
| Base fonctionnelle | Générateurs, électricité, irrigation, agriculture, serres, climatisation | Construction, survie | Prévu | Simulation et ressources cohérentes hors connexion |
| Structures avancées | Industrie, téléportation, stockage spécialisé, plateformes de montures | Construction, physique, réseau | Prévu | Mouvement, propriété et changement de région vérifiés |
| Espèces génériques | Stats, niveaux, alimentation, biomes, apparence, animations, ressources, sexe | Créatures, contenu | Prévu | Plusieurs espèces définies par données validées |
| Morphologies | Quadrupèdes, bipèdes, volants, aquatiques, insectoïdes, géants | Animation, physique, navigation | Prévu | Collider, locomotion et animation propres à chaque gabarit |
| Combat de créatures | Attaques, capacités, résistances, interaction terrain | Combat, créatures | Prévu | Cible hors portée refusée, cooldown et coût vérifiés |
| IA écologique | Errance, nourriture, chasse, fuite, embuscade, territoire, aggro, meute | IA, perception | Prévu | Scénarios reproductibles, budgets de simulation mesurés |
| IA tactique | Protection, choix de cible, réaction dégâts, défense, ordres | IA, combat, tribus | Prévu | Priorités et permissions vérifiées |
| Navigation | Sol, vol, eau, boss, cache et recherche asynchrone | Navigation, voxel | Prévu | Terrain modifié invalide les chemins concernés |
| Apprivoisement | KO, passif, spécial, torpeur, préférence, kibbles, efficacité, niveaux bonus | Taming, survie | Prévu | Méthodes et exceptions sourcées par espèce, persistance |
| Gestion des apprivoisés | Nourrissage, narcotiques, selles, monte, ordres, groupes, stockage | Créatures, inventaire, réseau | Prévu | Créature et inventaire conservés après redémarrage |
| Reproduction | Accouplement, œufs, gestation, incubation, température, croissance, nourriture | Élevage, survie | Prévu | Timers persistants, conditions et interruptions testés |
| Génétique | Héritage stats/couleurs, mutations, compteurs parents, lignées | Élevage, données | Prévu | Règles numériques vérifiées, tests statistiques avec seuils |
| Imprinting | Empreinte, bonus cavalier, spécialisations et conservation | Élevage, montures | Prévu | Bonus bornés, propriété et reconnexion testées |
| Progression PvE | Grottes, artefacts, obélisques, invocations, boss, arènes, difficultés, récompenses | Progression, combat | Prévu | Une route de progression jusqu'au boss jouable et persistante |
| Monde et société | Ascension, éventuel intermonde, tribus, alliances, PvE/PvP configurable | Réseau, progression | Prévu | Règles de serveur, migrations et ownership testés |

Avant chaque nouvelle tranche, scinder la ligne concernée en critères exécutables, ajouter une référence datée et distinguer implémentation, tests automatisés, essai manuel et mesure. Les règles numériques devront rester configurables.
