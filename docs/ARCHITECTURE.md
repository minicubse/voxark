# Architecture de la première vallée

| Crate | Responsabilité et dépendances |
| --- | --- |
| `primal-voxel` | Coordonnées entières, génération, stockage en chunks, différences et maillages. Indépendant de la fenêtre et du disque. |
| `primal-simulation` | Déplacement, contacts, visée, validation d'une intention de pose ou de destruction. Dépend du monde. |
| `primal-storage` | Lecture et écriture du monde versionné, verrou interprocessus et détection de corruption. Dépend du monde. |
| `primal-client` | Transforme les entrées en intentions, fait avancer la simulation, dessine les chunks et présente les erreurs de sauvegarde. Seule crate graphique. |

## Monde et rendu

La vallée couvre 128 × 64 × 128 cellules. Les axes X/Z vont de −64 à 63 inclus et Y de 0 à 63. La génération utilise de l'arithmétique entière. Les 256 chunks de 16³ sont résidents. Chaque cellule possède un octet de matériau. La copie de référence sert à retirer une différence lorsqu'une édition restaure le bloc initial.

Le mesher de référence teste les six voisins dans le monde entier. Le mesher greedy fusionne, dans un même chunk, les rectangles coplanaires de même matériau et même normale. Un test décompose ces rectangles et compare chaque face unitaire au résultat de référence. Lors d'une édition sur une frontière, l'invalidation inclut le voisin qui partage la face. Le client groupe les quadrilatères en maillages indexés et écarte les chunks hors du champ de vision.

Le catalogue `content/blocks.tsv` contient les noms et couleurs des sept matériaux. Il est validé et embarqué à la compilation. Le remplacement du contenu passe actuellement par une reconstruction du jeu. Une empreinte sur le volume entier de la seed 42 protège la compatibilité du générateur v1.

## Format PVXD v1

Une sauvegarde conserve la seed et les différences normalisées par rapport au générateur v1. Les entiers multi-octets sont little-endian.

| Octets | Contenu |
| --- | --- |
| 0–3 | Signature ASCII `PVXD` |
| 4–7 | Version de format puis version du générateur, deux `u16` |
| 8–15 | Seed `u64` |
| 16–19 | Nombre N de modifications, `u32` |
| 20–(20+4N−1) | N enregistrements de quatre octets. X, Y et Z signés puis identifiant de bloc non signé. Ordre lexicographique par (X,Y,Z). |
| 20+4N–23+4N | CRC-32 de tout ce qui précède |

Le lecteur limite la taille avant décodage. Il contrôle signature, versions, CRC, nombre, ordre, bornes et matériau de chaque édition. Une cellule restaurée au bloc d'origine disparaît de la sauvegarde. Une version inconnue provoque une erreur explicite. La v1 est la première version, il n'existe pas encore de migration.

Le client prend un verrou de fichier pour la session. Le fichier de verrouillage peut subsister après fermeture, le système libère le verrou à la fin du processus. Chaque écriture utilise un fichier temporaire dans le répertoire cible, synchronise ses données puis remplace le snapshot. Le CRC sert à détecter les dégâts accidentels. Seul le terrain est persistant dans cette tranche.

## Frontière du futur serveur

Le client propose une direction et une action. La simulation calcule la cellule atteinte et examine portée, matériau et occupation. Un accumulateur fait avancer le jeu à pas fixe, par défaut 60 Hz. Pour le multijoueur, le serveur devra posséder l'état du joueur, ordonner les commandes par tick, diffuser les éditions acceptées et renvoyer l'état à la reconnexion. Le transport, l'identité réseau et la réplication restent à implémenter.
