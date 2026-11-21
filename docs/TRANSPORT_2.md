# Transport LAN — étape 2

Choix vérifié le 21 novembre 2026 : `renet = 2.0.1` et `renet_netcode = 2.0.1`, versions exactes dans `primal-net` et le verrou Cargo. Le client Macroquad et le serveur synchrone utilisent le même transport Rust, sans intégration Bevy ni runtime asynchrone.

## Comparaison des options

| Option | Maintenance et compatibilité constatées | Coût pour ce projet |
| --- | --- | --- |
| Renet + renet_netcode | La version 2.0 a retiré les dépendances Bevy du cœur. La publication 2.0.1 du 30 octobre 2026 corrige les statistiques de débit. Les canaux transportent des messages fiables ordonnés ou non fiables; le backend netcode utilise UDP. [Releases officielles](https://github.com/lucaspoffo/renet/releases), [documentation Renet 2.0.1](https://docs.rs/renet/2.0.1/renet/) | Adapté à la boucle explicite poll/send/flush, fragmentation et retransmission déjà présentes. Compatibilité vérifiée avec Rust 1.100.0 Windows GNU par compilation et test UDP. |
| Quinn | Implémentation QUIC en Rust, API async, streams fiables et datagrammes applicatifs non fiables; tests annoncés sur Windows, Linux et macOS. L'historique de releases comporte maintenance et correctifs de sécurité. [README officiel](https://github.com/quinn-rs/quinn), [releases officielles](https://github.com/quinn-rs/quinn/releases) | Alternative crédible pour un produit Internet. Il faudrait intégrer l'exécution async, le cadrage des messages sur streams et la gestion des certificats. Cette appréciation du travail d'intégration est propre à notre architecture; aucun benchmark comparatif n'a été réalisé. |
| Lightyear | Bibliothèque de jeu pour Bevy, avec réplication, prédiction et interpolation. Le tableau officiel associe Lightyear 0.31 à Bevy 0.20; ses releases suivent les versions de Bevy. [README et tableau officiels](https://github.com/cBournhonesque/lightyear), [releases officielles](https://github.com/cBournhonesque/lightyear/releases) | Fonctions intéressantes mais dépendance à Bevy peu adaptée au client Macroquad actuel; migration d'architecture nécessaire. Choix à réévaluer si le moteur client change. |

Les dates/versions sont une observation datée, sans garantie de maintenance future. Les trois projets ont des sources et historiques publics; le choix Renet repose sur l'intégration légère et la compatibilité démontrée ici, sans prétendre qu'il est plus rapide que les autres.

## Canaux et limites

| Canal | Livraison | Mémoire de canal par connexion et par direction |
| --- | --- | --- |
| Control (0) | fiable ordonné, retransmission 100 ms | 2 Mio |
| Chunks (1) | fiable ordonné, retransmission 100 ms | 4 Mio |
| Edits (2) | fiable ordonné, retransmission 100 ms | 1 Mio |
| Input (3) | non fiable, sans garantie d'ordre | 64 Kio |
| Snapshots (4) | non fiable, sans garantie d'ordre | 256 Kio |

Ces plafonds configurent les buffers Renet d'émission et de réception; ils ne bornent pas toute la mémoire du processus, ni les buffers UDP du système. Le budget de packetization est 256 Kio par poll. `send` vérifie la place disponible avant de confier le message à Renet : `QueueFull(channel)` fournit une contre-pression explicite au lieu de provoquer la déconnexion Renet sur dépassement d'un canal fiable. Le producteur conserve et réessaie le message, ou diffère sa production. Une émission acceptée doit encore être transmise et acquittée.

Le wrapper refuse les messages fiables de plus de 256 Kio et les non fiables de plus de 1100 octets. Le codec gameplay impose sa limite plus stricte. Les événements serveur sont exposés par une file de 128 entrées : il faut appeler `next_event` jusqu'à `None` à chaque tour. Un poll avec cette file pleine retourne `EventQueueFull` avant de lire de nouveaux datagrammes.

Control transporte la connexion, la confirmation Ready et les résultats d’édition. Edits porte les commandes d’édition. Chunk et Forget partagent le canal Chunks, fiable et ordonné, y compris lors d’une revisite. Les entrées et les snapshots de joueurs utilisent Input et Snapshots. Les messages anciens y sont écartés par séquence ou par tick.

## API et identité

`ClientTransport::connect(bind, server, ConnectionId)` et `ServerTransport::bind(bind, max_clients)` créent les sockets; le serveur accepte 1 à 64 clients. `local_addr()` donne l'adresse effectivement liée, y compris avec un port demandé à zéro. Le tour normal est `poll(dt)` → vider `next_event` (serveur) → vider `recv` → `send` → `flush`. Utiliser le temps écoulé réel depuis le tour précédent comme `dt`. Le client n'accepte `send` qu'après `is_connected()`.

`ConnectionId::random()` fournit un identifiant éphémère. Il faut en créer un à chaque tentative, sans le dériver du token joueur. Le token persistant de 32 octets appartient au Hello gameplay et sert à retrouver le joueur; l'identifiant transport désigne uniquement la connexion en cours. Cette séparation permet une reconnexion sans confondre deux sessions réseau.

`ServerEvent::Connected(id)` / `Disconnected(id, reason)` et `ClientTransport::disconnect_reason()` exposent les transitions. `send` retourne taille, connexion absente ou contre-pression; `poll` retourne les erreurs de réception et netcode. Le `flush` client remonte les erreurs UDP d'émission. **Limite upstream : le `send_packets` serveur de renet_netcode 2.0.1 journalise les erreurs d'émission et ne les retourne pas.** Son wrapper `flush()->Result` retourne donc `Ok(())` après cet appel; ce résultat ne prouve pas la livraison. [Source officielle du backend serveur](https://docs.rs/renet_netcode/2.0.1/src/renet_netcode/server.rs.html).

Le mode `Unsecure` est celui du prototype LAN de confiance : aucun service d'émission de connect-token authentifié n'est déployé. Le token joueur de ce prototype ne constitue pas une authentification Internet. [Authentifications netcode officielles](https://docs.rs/renet_netcode/2.0.1/renet_netcode/enum.ClientAuthentication.html).

## Mesures et vérification

`stats()` compte exactement les octets et messages applicatifs acceptés à l'émission, livrés par `recv`, ainsi que les refus pour contre-pression. Ces valeurs ne sont pas des paquets UDP. `rtt()` et `network_info()` donnent les estimations Renet de RTT, perte et débits des paquets Renet, retransmissions comprises, hors enveloppes netcode/UDP/IP. Les compteurs UDP cumulés ne sont pas accessibles via la socket privée du backend officiel; aucune estimation de paquets n'est présentée comme un compteur réel.

Les deux tests `crates/net/tests/transport.rs` passent avec le vrai backend UDP. Le premier relaie exclusivement les datagrammes légitimes de ses deux endpoints loopback, sans altération du contenu, avec 30 ms de délai simulé par sens et suppression déterministe d'un datagramme sur sept. Il vérifie les 32 messages fiables dans l'ordre, un chunk fragmenté de 16 Kio identique après réception, le passage d'inputs/snapshots non fiables, le RTT positif et les compteurs applicatifs. Le relais mesure réellement les longueurs et nombres de datagrammes reçus : lors de la validation initiale, **2359 paquets, 157503 octets UDP utiles et 337 pertes simulées**, RTT estimé **70,6 ms**. Ce résultat dépend de l'exécution; il ne mesure ni le débit maximal ni la latence d'un LAN réel.

Le second remplit légitimement le buffer d'émission fiable sans flush jusqu'à `QueueFull`, vérifie que la connexion reste ouverte, puis les erreurs de taille et de destinataire absent. Aucun test offensif ni trafic vers une machine externe.

```powershell
. tools/env-windows.ps1 -Cache ../../work
$env:CARGO_TARGET_DIR = [IO.Path]::GetFullPath('../../work/target-2')
cargo test -p primal-net --test transport -- --nocapture
```
