# Validation multijoueur sur Ryzen 7 7730U

Deux vrais clients graphiques Macroquad et un serveur dédié Windows ont tourné simultanément sur le même Ryzen 7 7730U (8 cœurs, 16 processeurs logiques, Radeon intégrée). Les essais réseau à quatre joueurs utilisent quatre processus de sonde indépendants. Le bi-Xeon n’a pas été testé.

## Deux fenêtres de jeu

Trois répétitions en 1280 × 720, seed 20261120, greedy, rayon de rendu demandé 4. Chaque fenêtre suit un parcours de vol et reçoit les déplacements de l’autre. Par fenêtre, 120 images d’échauffement et 600 images mesurées. La capture montre le second joueur en jeu. Les temps de tick comprennent simulation, streaming et traitement réseau, hors sommeil du régulateur. Le serveur enregistre les percentiles par fenêtre de 120 ticks. La mémoire rapportée est le maximum des échantillons du working set toutes les 250 ms. [Rapports et empreintes](../benchmarks/results/2/graphics/) · [Capture d’un joueur distant](../benchmarks/results/2/graphics/final-3-a-peer.png)

| Répétition | FPS A / B | Frametime P99 A / B (ms) | RTT moyen A / B (ms) | CPU serveur (s) | Pic RAM serveur (Mio) |
|---|---:|---:|---:|---:|---:|
| 1 | 119,8 / 119,4 | 10,8 / 11,1 | 10,1 / 10,0 | 0,34 | 10,10 |
| 2 | 117,5 / 116,7 | 15,9 / 15,6 | 16,6 / 16,6 | 0,64 | 10,12 |
| 3 | 118,4 / 117,3 | 14,6 / 14,7 | 14,6 / 14,8 | 0,42 | 10,24 |

Sur les neuf fenêtres de mesure où deux joueurs sont connectés, le serveur atteint **60,0 Hz** à la précision affichée. Le P95 du tick varie de 0,92 à 1,74 ms et son P99 de 0,99 à 1,88 ms. Le P95 du streaming varie de 0,52 à 1,03 ms. Le serveur atteint 1 024 chunks résidents. Le RTT comprend l’ordonnancement des processus sur la boucle locale. Les 600 échantillons par fenêtre et les trois répétitions décrivent ce parcours court.

## Réseau, intégrité et reprise

Les clients de sonde utilisent la bibliothèque réseau du jeu et des identités distinctes. Le test vérifie l’état de leurs répliques : apparition mutuelle, déplacement observé par un tiers, destruction, construction, rejet d’une cible invalide et d’une ancienne révision, un seul gagnant lors d’une édition concurrente. Deux joueurs visitent des zones distantes de 4 096 blocs, inversent leurs zones, puis reviennent. Les tests attendent l’arrivée du chunk attendu et le retrait du chunk distant. Après reconnexion, ils vérifient l’identité. Après redémarrage, ils vérifient le terrain et le premier état reçu du joueur avant toute téléportation administrative.

| Parcours | Processus clients | Datagrammes reçus par les relais | Pertes injectées | État final |
|---|---:|---:|---:|---|
| Local | 2 | 2 762 | 0 | Convergence |
| Local, deux groupes | 4 | 10 959 | 0 | Convergence |
| Retard configuré 80 ms, probabilité de perte 8 % | 2 | 5 185 | 434 | Convergence |

Sur le premier relais du parcours à deux en local, 189,1 datagrammes/s et 6 084 octets/s arrivent du client, 68,2 datagrammes/s et 4 462 octets/s du serveur. Les mesures sont des charges utiles UDP, avec les retransmissions et sans les en-têtes IP. Le détail de chaque direction, durée et reconnexion figure dans les rapports. Ces transferts incluent les phases de chargement et de déplacement. [Deux clients](../benchmarks/results/2/network/two.json) · [Quatre clients](../benchmarks/results/2/network/four.json) · [Pertes et retard](../benchmarks/results/2/network/loss.json)

Le test envoie l’arrêt dès que le serveur signale l’envoi de l’édition au worker disque, puis relit le bloc au redémarrage. L’écriture peut déjà être terminée lorsque le serveur traite la commande console. Des tests Rust distincts arrêtent un sous-processus à des étapes précises du journal et pendant son rejeu. Ils vérifient aussi l’atomicité des mises à jour d’inventaire et leur idempotence. Une coupure physique du poste reste hors de ces essais.

Deux erreurs de scénario ont été corrigées pendant les essais. À quatre clients, un joueur stationné au point de départ occupait la cellule à reconstruire ; le serveur a correctement refusé la pose. Sous délai réseau, une commande de déplacement pouvait encore arriver après la commande console de téléportation. Le banc confirme maintenant la prise en compte de l’entrée neutre avant de déplacer le joueur.

## Choix du format des chunks

Le serveur envoie la seed, vérifie la version du générateur, puis transmet soit les différences indexées, soit les 4 096 matériaux. La décision utilise la taille encodée des différences. Mesure release sur un chunk d’air en altitude, 300 répétitions par représentation, allocations et publication dans une nouvelle réplique incluses.

| Cellules modifiées | Octets différences / complet | Décodage, génération et installation (µs), différences / complet |
|---|---:|---:|
| 0 | 7 / 4 104 | 1,1 / 18,9 |
| 100 | 207 / 4 104 | 7,2 / 24,3 |
| 1 400 | 4 080 / 4 104 | 193,9 / 183,3 |
| 2 048 | 6 024 / 4 104 | 292,3 / 303,9 |
| 4 096 | 12 168 / 4 104 | 642,1 / 610,2 |

Le gain de bande passante est important sur les chunks peu édités. Proche du seuil, le format compact peut être légèrement plus lent à reconstruire. Le transfert complet conserve la vérification du générateur pour maintenir une base identique à la sauvegarde régionale. [Mesures individuelles](../benchmarks/results/2/network/chunk-wire.csv) · [Microbenchmark](../crates/net/examples/chunk_wire.rs)

## Solo, périmètre et reproduction

Un essai depuis une extraction neuve du ZIP a également démarré le serveur et le client par le lanceur d’hébergement. Le serveur a confirmé la présence du joueur, puis son arrêt après fermeture de la fenêtre. Les 65 tests 1C sont conservés. Le workspace compte 87 tests Rust réussis. Le test Windows a détruit puis reposé un bloc, confirmé F5, fermé le client et relu un ancien monde. En solo, sur un passage supplémentaire de 4,22 km au rayon 2, le binaire livré affiche 116,2 FPS, un P99 de 16,59 ms et quatre images sans sol sur 720. Ce contrôle isolé ne démontre pas une équivalence générale des performances avec 1C. [Résultat solo](../benchmarks/results/2/solo/greedy/) · [Comparaisons 1C](STREAMING_1C.md)

Pour reproduire les parcours réseau, construire les binaires en release ainsi que la sonde, puis suivre les commandes de son README. Pour le parcours graphique, passer les deux exécutables Windows à `tools/test-net-graphics.ps1 -Server … -Client … -Repetitions 3`. Le serveur vise 60 Hz par défaut et envoie les snapshots de joueurs tous les trois ticks. Le protocole v1 assure le LAN de confiance. Les scénarios WAN public, la charge de longue durée et les performances sur la workstation bi-Xeon restent à mesurer. [Sonde multiprocessus](../tools/net-probe/README.md) · [Banc graphique](../tools/test-net-graphics.ps1) · [Guide du serveur](MULTIJOUEUR_2.md)
