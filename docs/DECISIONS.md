# Décisions techniques

## D001 — Noyau autonome et premier client

Décision du 20 novembre 2026. Le monde, les collisions, les commandes et la persistance restent dans des crates Rust indépendantes du rendu. Le premier client utilise Macroquad pour obtenir une référence exécutable et instrumentée. Bevy avec sous-système voxel autonome reste le candidat pour le rendu et l'animation à long terme. Ce choix de livraison ne démontre aucun avantage de performance de Macroquad sur Bevy.

Macroquad fournit fenêtre, entrée, caméra et dessin avec peu de dépendances. Son manifeste déclare MIT OR Apache-2.0. Le client pourra être remplacé en conservant le monde et les tests. La version réellement compilée est celle de Cargo.lock. [Dépôt](https://github.com/not-fl3/macroquad), [manifeste](https://github.com/not-fl3/macroquad/blob/master/Cargo.toml).

## D002 — Comparaison des fondations

Les coûts ci-dessous sont des implications d'architecture à vérifier. Aucune comparaison CPU, GPU ou mémoire entre moteurs n'a été exécutée pendant cette décision.

| Critère | A — Bevy + voxel autonome | B — wgpu + moteur dédié | C — base existante |
| --- | --- | --- | --- |
| CPU | ECS parallèle, coût de scheduling et extraction à mesurer | Budget de simulation et soumissions contrôlé, tout à implémenter | Dépend du moteur et du contenu hérité |
| GPU | Pipeline 3D configurable, tester les options Low | Pipeline minimal possible, maintenance des shaders et passes | Réutilisation possible, exigences héritées à vérifier |
| Mémoire | Actifs, ECS et buffers à profiler | Allocations contrôlables, responsabilité complète | Auditer caches, représentation voxel et durée de vie |
| Qualité visuelle | Éclairage, matériaux et pipeline d'animation intégrés | Développer matériaux, éclairage, animation et outils | Veloren fournit un jeu visuellement cohérent, plugin voxel plus ciblé |
| GPU Radeon intégré | Tester backend et limites avec preset Low | Tester les adaptateurs wgpu et prévoir les fallbacks | Tester chaque base sur la machine cible |
| Génération et meshing | Jobs et mesher métier à connecter | Ordonnanceur, jobs et uploads à écrire | Plugin voxel fournit une base plus proche du terrain |
| Physique | Avian ou intégration Rapier, collisions voxel spécifiques | Rapier indépendant ou collisions spécialisées | Évaluer adéquation et couplage du système hérité |
| Créatures et animation | Import et animation disponibles | Pipeline complet à construire | Veloren possède son propre pipeline, adaptation importante |
| Réseau | Transport, autorité et réplication à intégrer | Même travail, protocole entièrement maîtrisé | Veloren a un client/serveur, plugin voxel ne livre pas le jeu réseau demandé |
| Modularité | Plugins et crates, contenir la dépendance graphique | Frontières libres, vérifier la cohérence | Couplage à mesurer avant adoption |
| Maintenance | Écosystème et mises à jour d'API à suivre | Charge d'entretien maximale pour une petite équipe | Coût des divergences avec l'amont |
| Évolutivité | Bonne base pour ajouts métiers, budgets à prouver | Adaptation fine, risque de multiplier les systèmes internes | Dépend de la proximité du projet avec Primal Voxel |
| Complexité | Intermédiaire | Élevée | Faible pour un exemple, potentiellement élevée pour une conversion complète |

Bevy documente ECS, rendu 3D, scènes et animation. wgpu expose une abstraction graphique portable et laisse les systèmes de jeu à l'application. Cette comparaison justifie de conserver A comme candidat de production et d'éviter une décision irréversible avant mesures. [Bevy](https://bevy.org/), [wgpu](https://wgpu.rs/).

### Bases examinées et licences

| Base | Constat vérifié le 20 novembre 2026 | Conséquence |
| --- | --- | --- |
| Bevy | MIT OR Apache-2.0 | Candidat, aucune dépendance Bevy nécessaire au serveur |
| wgpu | MIT OR Apache-2.0 | Candidat pour un futur pipeline spécialisé |
| bevy_voxel_world | Plugin Bevy, MIT OR Apache-2.0, exemples de meshing et persistance | Comparer sur la même scène avant une intégration |
| Veloren | RPG voxel Rust avec architecture client/serveur, fichier LICENSE GPL v3 | Étudier les concepts, déterminer les obligations avant toute reprise de code |
| Horizon | Dépôt officiel identifié, README courant annonce LGPL v3 ou ultérieure pour le travail original, historique MIT mentionné | Fixer commit et licence du fichier avant toute reprise |
| GeminiIsland | Démonstration Google de construction voxel collaborative par instruction | Inspiration de construction, code et licence réutilisable non établis |

Les manifestes consultés sont des branches amont évolutives. La distribution Windows livre les notices de 37 paquets Cargo et des composants de runtime dans LICENCES/INDEX.txt. [Bevy](https://github.com/bevyengine/bevy/blob/main/Cargo.toml), [wgpu](https://github.com/gfx-rs/wgpu/blob/trunk/Cargo.toml), [bevy_voxel_world](https://github.com/splashdust/bevy_voxel_world/blob/main/Cargo.toml), [Veloren](https://github.com/veloren/veloren), [licence Veloren](https://github.com/veloren/veloren/blob/master/LICENSE), [Horizon](https://github.com/horizon-rust/horizon), [annonce Google](https://blog.google/innovation-and-ai/models-and-research/gemini-models/introducing-gemini-4-hydrogen/).

Les chiffres publiés par Horizon concernent son jeu, ses scénarios et son matériel. Ils ne constituent aucune mesure de Primal Voxel. Le dépôt documente notamment un banc d'essai sur matériel haut de gamme, différent du portable cible. [README Horizon](https://github.com/horizon-rust/horizon/blob/main/README.md).

## D003 — Physique du premier monde

Le contrôleur utilise des collisions AABB contre les cellules solides, un pas fixe et un raycast voxel. Cela fournit une référence testable pour marche, saut et édition. Les corps rigides, montures, plateformes mobiles, projectiles et contacts complexes nécessiteront une extension.

Avian s'intègre directement aux composants et systèmes Bevy et propose requêtes spatiales, corps rigides, CCD et interpolation. Licence MIT OR Apache-2.0. Rapier fournit une API physique Rust indépendante et une intégration Bevy, avec option de déterminisme. Licence Apache-2.0. Évaluer les deux sur contacts, gros colliders voxel reconstruits et serveur headless avant choix. [Avian](https://github.com/avianphysics/avian), [Rapier](https://github.com/dimforge/rapier/blob/master/website/docs/about_rapier.mdx).

## D004 — Référence voxel et protocole de comparaison

Premier format dense et borné avec chunks de 16³. Premier mesher à faces exposées et invalidation des chunks voisins après édition. La sauvegarde contient la seed, une version et les différences du terrain. Les budgets de streaming et la compression seront introduits sur une référence correcte.

Pour comparer 16³, 32³ et 64³, garder seed, volume total et modifications identiques. Mesurer génération, remeshing après édition en bordure, mémoire maximale et uploads. Comparer cubes complets, faces exposées puis greedy sur terrain naturel, plateau, grotte et construction dense. Une réduction du nombre de faces constitue une mesure géométrique, elle ne prouve aucun gain FPS.

Le futur essai A/B doit utiliser le même terrain exporté, la même caméra, le même viewport, le même shading et les mêmes étapes d'édition. Conserver versions, mode release, pilote, backend, durée, échauffement, FPS moyens, lows 1 % et 0,1 %, percentiles de frametime, mémoire et pics de remeshing. Profiler avant de changer de moteur. Aucun résultat A/B acquis à ce stade.

## D005 — Contenu original et autorité

Les familles ARK et Primal Fear décrivent des objectifs de mécaniques. Leurs noms servent aux matrices de référence. Les futurs espèces, ressources, apparences et capacités du jeu auront leurs propres définitions et identités.

Le client solo appelle le noyau de simulation. Le futur serveur sera seul autorisé à valider les commandes, modifier le monde et enregistrer les progrès. Le partage d'un noyau sans dépendance graphique prépare cette intégration. La réplication réseau reste un jalon distinct à valider avec plusieurs processus.
