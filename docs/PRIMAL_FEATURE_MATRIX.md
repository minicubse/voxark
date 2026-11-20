# Matrice de référence Primal Fear

Consultation du 20 novembre 2026. Références primaires Primal Fear, Workshop 839162288, page mise à jour le 6 juin 2025, et Boss Expansion, Workshop 899987403, page mise à jour le 20 juin 2024. Ces dates identifient les descriptions consultées, elles ne constituent pas une version installée ou testée. [Primal Fear](https://steamcommunity.com/sharedfiles/filedetails/?id=839162288), [Boss Expansion](https://steamcommunity.com/sharedfiles/filedetails/?id=899987403).

La page de Primal Fear décrit un contenu PvE et plusieurs catégories jouables. Elle annonce aussi des objets de soin, tranquillisants, sang et armures. La page désigne le travail sous CC BY-NC-ND 4.0. Le projet crée ses propres systèmes et contenus. [Description des auteurs](https://steamcommunity.com/sharedfiles/filedetails/?id=839162288).

Toutes les lignes sont prévues et absentes du prototype initial. « À vérifier » indique que les détails de référence dépassent les descriptions primaires consultées. Les catégories servent à inventorier les objectifs, leur ordre ne fixe aucun graphe de progression.

| Famille ou contenu demandé | Référence et niveau de vérification | Système générique prévu | Preuve exigée |
| --- | --- | --- | --- |
| Toxic | Catégorie citée dans la description des auteurs | Stats, statuts, nourriture, résistance | Variante originale testée et progression accessible |
| Alpha | Catégorie citée | Profils de stats et récompenses | Courbes et coûts validés |
| Apex | Catégorie citée | Rôles de combat et capacités | Scénario tactique reproductible |
| Fabled | Catégorie citée | Récolte et utilité | Rendements et choix de progression vérifiés |
| Elemental | Catégorie citée | Effets, résistances et capacités | Matrice d'interactions testée |
| Advanced Elemental | Détails à vérifier | Composition de capacités | Combinaisons valides et coûts bornés |
| Omega, Black Omega | Détails et appartenance de version à vérifier | Variantes et conditions | Définition sourcée, progression sans blocage |
| Celestial, Demonic | Catégories citées | Capacités, coûts, contre-jeu | Choix tactiques, cooldowns, immunités vérifiés |
| Chaos, Spirit | Cités pour les boss de Boss Expansion | Profils de combat et invocations | Tests des phases et contre-jeu |
| Primal, Origin | Boss cités dans Primal Fear | Boss, récompenses et accès | Combat complet et progression persistante |
| Buffoon, Elder | Catégories citées | Comportements et capacités | Définition source précisée avant implémentation |
| Primal Tek et autres catégories | Liste exhaustive à vérifier par version/extension | Modules de contenu déclaratifs | Références résolues, contenu original validé |
| Nourritures et kibbles | Demande utilisateur, règles exactes à vérifier | Recettes, consommation, apprivoisement | Graphe des ressources et accessibilité vérifiés |
| Tranquillisants, narcotiques, potions, soins | Système d'objets décrit, variantes à vérifier | Objets et effets temporisés | Coûts, durée, stacking et bornes testés |
| Équipements et armures | Famille décrite, valeurs à vérifier | Équipements, résistances et durabilité | Équilibrage contre plusieurs rôles |
| Sang, âmes, trophées, ressources uniques | Sang décrit, autres détails à vérifier | Tables de butin et recettes | Production/consommation sans cycle bloquant |
| Stations, déblocages et invocations | Exigences utilisateur, détails à vérifier | Graphe de progression et recettes | Dépendances valides, chemins jusqu'aux boss |

La liste et les distinctions confirmées ci-dessus proviennent des pages d'auteurs. Aucun multiplicateur publié n'est adopté comme constante du jeu. [Primal Fear](https://steamcommunity.com/sharedfiles/filedetails/?id=839162288), [Boss Expansion](https://steamcommunity.com/sharedfiles/filedetails/?id=899987403).

## Capacités et boss à spécifier

| Ensemble demandé dans la mission | État de référence | Système prévu | Validation avant clôture |
| --- | --- | --- | --- |
| Feu, foudre, explosions, météores, rayons, attaques de zone | Détails par créature à vérifier | Géométrie d'attaque, dégâts, télégraphie | Portée, obstacle, coût et cadence autoritaires |
| Auras, boucliers, soins, régénération, immunités | Détails par version à vérifier | Modificateurs et statuts | Stacking, expiration et absence d'invulnérabilité accidentelle |
| Brûlure, gel, poison, torpeur, saignement | Règles numériques à vérifier | Effets temporisés | Résistances, périodicité et sauvegarde |
| Invocations, enragement, bonus élémentaires, passifs et actifs | Détails à vérifier | Conditions, coûts et événements | Limites d'entités et combinaisons cohérentes |
| Pouvoirs à plusieurs phases | Règles à vérifier | Machine à états de capacité | Transitions, interruption et réplication |
| Primal bosses, Origins | Catégories confirmées, comportements à vérifier | Arènes, phases, butin | Route complète et persistance |
| Emperor, Empress, Guardians Chaos/Spirit | Détails à vérifier | Boss configurables | Références datées et scénarios de combat |
| Creator | Nom confirmé, mécanique à vérifier | Boss configurable | Récompense et déblocage cohérents |
| Destroyer, Colossi, boss finaux | Présence/version et détails à vérifier | Boss configurables | Sources identifiées avant règles |
| Boss Expansion | Extension distincte confirmée, dépend de Primal Fear | Module de contenu avec dépendances | Validation du manifeste et tests de progression |

Boss Expansion distingue notamment boss Primal non apprivoisables et plusieurs autres catégories apprivoisables. Cette distinction appartient à la référence et devra faire l'objet d'une décision de design pour les boss originaux. [Page des auteurs](https://steamcommunity.com/sharedfiles/filedetails/?id=899987403).

Les futures validations de données doivent détecter références manquantes, valeurs hors limites, recettes inaccessibles, cycles involontaires de ressources, capacités incompatibles et boss impossibles à vaincre. Conserver les versions de contenu et paramètres de serveur avec les sauvegardes et les résultats d'essai.
