# Architecture du monde voxel

| Crate | Responsabilité et dépendances |
| --- | --- |
| `primal-voxel` | Coordonnées entières, génération, stockage en chunks, différences et maillages. Indépendant de la fenêtre et du disque. |
| `primal-simulation` | Déplacement, contacts, visée, validation d'une intention de pose ou de destruction. Dépend du monde. |
| `primal-storage` | Lecture et écriture du monde versionné, verrou interprocessus et détection de corruption. Dépend du monde. |
| `primal-client` | Transforme les entrées en intentions, fait avancer la simulation, dessine les chunks et présente les erreurs de sauvegarde. Seule crate graphique. |

## Référence historique 1a — monde et rendu

La vallée couvre 128 × 64 × 128 cellules. Les axes X/Z vont de −64 à 63 inclus et Y de 0 à 63. La génération utilise de l'arithmétique entière. Les 256 chunks de 16³ sont résidents. Chaque cellule possède un octet de matériau. La copie de référence sert à retirer une différence lorsqu'une édition restaure le bloc initial.

Le mesher de référence teste les six voisins dans le monde entier. Le mesher greedy fusionne, dans un même chunk, les rectangles coplanaires de même matériau et même normale. Un test décompose ces rectangles et compare chaque face unitaire au résultat de référence. Lors d'une édition sur une frontière, l'invalidation inclut le voisin qui partage la face. Le client groupe les quadrilatères en maillages indexés et écarte les chunks hors du champ de vision.

Le catalogue `content/blocks.tsv` contient les noms et couleurs des sept matériaux. Il est validé et embarqué à la compilation. Le remplacement du contenu passe actuellement par une reconstruction du jeu. Une empreinte sur le volume entier de la seed 42 protège la compatibilité du générateur v1.

## Format historique PVXD v1

Une sauvegarde conserve la seed et les différences normalisées par rapport au générateur v1. Les entiers multi-octets sont little-endian.

| Octets | Contenu |
| --- | --- |
| 0–3 | Signature ASCII `PVXD` |
| 4–7 | Version de format puis version du générateur, deux `u16` |
| 8–15 | Seed `u64` |
| 16–19 | Nombre N de modifications, `u32` |
| 20–(20+4N−1) | N enregistrements de quatre octets. X, Y et Z signés puis identifiant de bloc non signé. Ordre lexicographique par (X,Y,Z). |
| 20+4N–23+4N | CRC-32 de tout ce qui précède |

Le lecteur limite la taille avant décodage. Il contrôle signature, versions, CRC, nombre, ordre, bornes et matériau de chaque édition. Une cellule restaurée au bloc d'origine disparaît de la sauvegarde. Une version inconnue provoque une erreur explicite. L’import v1 vers v2 est décrit plus bas.

Le client prend un verrou de fichier pour la session. Le fichier de verrouillage peut subsister après fermeture, le système libère le verrou à la fin du processus. Chaque écriture utilise un fichier temporaire dans le répertoire cible, synchronise ses données puis remplace le snapshot. Le CRC sert à détecter les dégâts accidentels. Seul le terrain est persistant dans cette tranche.

## Frontière du futur serveur

Le client propose une direction et une action. La simulation calcule la cellule atteinte et examine portée, matériau et occupation. Un accumulateur fait avancer le jeu à pas fixe, par défaut 60 Hz. Pour le multijoueur, le serveur devra posséder l'état du joueur, ordonner les commandes par tick, diffuser les éditions acceptées et renvoyer l'état à la reconnexion. Le transport, l'identité réseau et la réplication restent à implémenter.

## 1B.2 — Terrain et régions

Le constructeur de compatibilité charge la vallée historique. Le constructeur `World::streamed` démarre vide. Les clés des chunks utilisent la division euclidienne de coordonnées i32, sur 128 blocs de hauteur. Un chunk uniforme conserve un matériau. Un chunk hétérogène utilise 4 096 octets. Les différences mémorisent le matériau original des seules cellules modifiées. Les vues de maillage immuables partagent les données par Arc. Les éditions utilisent la copie sur écriture si une vue reste en vol. La physique considère un chunk absent comme un obstacle et la visée s’arrête avant la zone inconnue.

PVXD v2 conserve un manifeste et des régions de 4³ chunks dans le dossier voisin `<chemin>.regions`. Les coordonnées signées de la région et les positions locales identifient les blocs. Le lecteur vérifie taille, version, seed, identité et CRC. Un remplacement atomique publie les différences par région sous verrou. La migration construit une arborescence temporaire et la renomme après synchronisation. Le fichier v1 reste disponible à l’identique. La suite teste des interruptions de processus avant et après publication. La résistance à une coupure électrique dépend du système de fichiers et du matériel.

Sur la vallée de seed 42 en release, dix essais donnent une médiane de 2,09 ms pour la génération, 2,07 ms pour les faces exposées et 5,88 ms pour le greedy complet. Les voxels résidents représentent 536 701 octets de charge utile contre 2 097 152 octets précédemment. Les tables, allocations, diffs et maillages ont un coût supplémentaire. La fermeture de tous les sommets au-delà de la précision f32 exige une origine flottante future. Prochaine étape : connecter chargement et maillage aux workers. [Banc CPU](../benchmarks/results/1b/voxel-dynamique.txt)

## 1B.3 à 1B.5 — Exécution

La cinquième crate, `primal-streaming`, accepte un monde, un stockage, les budgets et une fonction de préparation CPU. Le fil propriétaire est le seul à valider les commandes et à publier un chunk. Un ticket de chargement est à usage unique. Les tableaux de voxels immuables et ceux des six voisins alimentent les travaux de meshing. La version de maillage change à l’édition d’une face voisine ainsi qu’à l’arrivée et à l’éviction du voisin. Les résultats reçus sont revérifiés avant publication. Un accusé de sauvegarde ne libère que la révision effectivement écrite.

Le plan retient un cache de résidents classés autour de la position courante et de la position anticipée. Les chunks sales sont épinglés. Les travaux de chargement ou de maillage obsolètes perdent leur droit de publication, même si un calcul déjà commencé se termine. Le nombre de tâches admises borne aussi les réponses accumulées. Chaque travail de maillage réserve 6 Mio du budget de préparation. Le budget de résidence réserve 64 Kio par slot et le budget des géométries publiées se base sur les capacités des tableaux de sommets et d’indices. Le fil graphique déplace les tableaux préparés dans des Mesh Macroquad. Les appels de dessin et la gestion du contexte graphique restent sur ce fil.

Un serveur sans fenêtre peut instancier le même monde et appeler la même validation de commande. Le test de traversée utilise le stockage régional et les workers sans charger de moteur graphique. La réplication et la synchronisation des joueurs restent à développer.
