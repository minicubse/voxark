# Serveur autoritaire et client Windows

## Jouer

Décompresser l’archive avant de lancer **Jouer.cmd**. Le menu propose **Solo**, **Héberger** et **Rejoindre**. Pour jouer à deux sur un réseau local, l’hôte lance **Heberger.cmd** ou utilise le menu. Le second joueur entre l’adresse IPv4 de l’hôte suivie du port UDP, par exemple `192.168.1.20:47620`. Pour deux clients sur un même poste, utiliser deux fichiers d’identité distincts avec `--identity`. Le même jeton permet de retrouver son joueur après reconnexion. Le serveur autonome se lance avec **Serveur.cmd**. Taper `stop` dans sa console pour enregistrer et quitter.

`server.json` configure le chemin du monde, l’adresse, le port, la seed des nouveaux mondes, le tickrate et la capacité. Le binaire seul écoute par défaut sur la boucle locale. Le fichier de lancement LAN écoute sur les interfaces réseau. Renet utilise ici le mode LAN Unsecure. Un déploiement Internet avec authentification reste à concevoir. Le menu affiche le nombre de joueurs et une estimation du RTT. En ligne, Échap libère la souris puis permet de quitter la session. [Options de transport](TRANSPORT_2.md)

## Chemin d’une commande

1. Le client envoie la version du protocole, la version du générateur et un jeton opaque de 32 octets. Le serveur refuse une version incompatible ou une identité déjà connectée. Il inscrit ou retrouve le joueur sur disque avant la bienvenue. Le client confirme que sa réplique est prête avant le premier chunk.
2. Le serveur reçoit des intentions séquencées. Un pas fixe fait avancer chaque joueur avec la même simulation Rust qu’en solo. Les entrées non finies ou hors bornes sont rejetées. Une rafale de commandes ne fait pas avancer plusieurs fois le temps de simulation. Un mouvement ancien expire. Les positions serveur alimentent le plan prédictif commun. Une arrivée attend les vrais voxels de collision. Les snapshots portent le tick et les clients écartent les anciens.
3. Une édition propose un matériau, une direction, une cellule et sa révision observée. Le serveur recalcule la visée depuis la position autoritaire, vérifie la portée, l’occupation et la présence des autres joueurs. Deux commandes concurrentes sur le même chunk et la même révision ne peuvent obtenir qu’un seul commit. Une zone autour du joueur est synchronisée par instantanés fiables. Les maillages sont calculés sur le client. La caméra locale et les représentations des joueurs distants lissent les positions entre snapshots.
4. Les chargements de plusieurs centres partagent les mêmes 4 workers, 24 travaux et 1 024 chunks résidents effectifs par défaut. Le serveur n’envoie que les chunks utiles à chaque connexion et retire ceux qui ont quitté son intérêt. Le canal fiable de chunks maintient l’ordre entre retrait et revisite. Les joueurs possèdent des canaux séparés pour contrôle, éditions, entrées et snapshots. [Implémentation du serveur](../crates/server/src/room.rs) · [Protocole](../crates/net/src/messages.rs)

## Disque et reprise

Une file bornée confie les écritures à un worker dédié. Chaque édition en attente épingle son chunk et réserve la cellule de placement. La transaction redb enregistre l’identifiant d’opération, la révision et les différences destinées aux régions. Le worker applique le journal aux fichiers régionaux puis confirme l’édition. À la réouverture, il rejoue les écritures interrompues avant de servir la zone. L’identifiant d’opération protège les retries. La transaction sait aussi associer terrain, état joueur et futur contenu d’inventaire. Les tests couvrent le rejet intégral d’une transaction et plusieurs interruptions forcées entre journal, régions et nettoyage. [Tests de transactions](../crates/ledger/tests/transactions.rs) · [Tests de reprise](../crates/ledger/src/lib.rs) · [Arrêt du worker](../crates/server/src/disk.rs)

Les profils actifs sont enregistrés périodiquement, aux déconnexions et à l’arrêt propre. Un arrêt brutal peut faire perdre le déplacement depuis le dernier enregistrement. Pour déplacer un serveur, l’arrêter puis copier **tout** le dossier `.pvx.regions`, y compris `server.redb`. Conserver séparément le jeton de chaque joueur. L’import du format v1 conserve le fichier original. Le mode solo historique reste disponible.

## Étape suivante

Mesurer le même parcours sur la workstation bi-Xeon. Pour le premier prototype de créature, ajouter une identité d’entité et une espèce au modèle réseau, une IA à tick fixe côté serveur et une représentation interpolée côté client. Vérifier qu’une créature sort de l’intérêt d’un joueur, y revient et survit à un redémarrage. Santé, combat, torpeur, apprivoisement, montures, inventaires, élevage, mutations, boss et pouvoirs pourront ensuite utiliser ce modèle d’entités et les transactions durables. Ces mécaniques restent à développer.

[Mesures et limites du jalon 2](PERFORMANCE_2.md)
