# Sonde réseau multiprocessus

La sonde utilise `primal-net`, `Replica` et le raycast de `primal-simulation`.
Python lance un véritable serveur et un processus indépendant par client.
Les sorties JSONL incluent les identifiants, révisions et résultats des éditions.
Le rapport ne déclare la convergence qu'après observation dans les répliques.

Depuis la racine du dépôt, dans PowerShell :

```powershell
. ./tools/env-windows.ps1 -Cache ../../work
$env:CARGO_TARGET_DIR = '../../work/target-2'
cargo build --locked -p primal-server
cargo build --locked --manifest-path tools/net-probe/Cargo.toml
python tools/test-multiplayer.py --server ../../work/target-2/debug/primal-server.exe --probe ../../work/target-2/debug/primal-net-probe.exe --output ../../work/multiplayer-clean
python tools/test-multiplayer.py --server ../../work/target-2/debug/primal-server.exe --probe ../../work/target-2/debug/primal-net-probe.exe --output ../../work/multiplayer-lossy --loss .08 --delay-ms 80
python tools/test-multiplayer.py --server ../../work/target-2/debug/primal-server.exe --probe ../../work/target-2/debug/primal-net-probe.exe --output ../../work/multiplayer-four --clients 4 --loss .08 --delay-ms 80
```

Le dossier de sortie doit être nouveau. Il est conservé, monde et identités
compris. Le serveur doit prendre `--world`, `--bind`, annoncer l'événement
`started` en JSONL et accepter sur stdin `teleport ID X Y Z` et `stop`.
Les joueurs doivent conserver leur identifiant après reconnexion avec le même
jeton. Les snapshots `Players` doivent inclure tous les joueurs connectés.

Le test arrête le serveur immédiatement après une édition acceptée, puis
vérifie la transaction après redémarrage. Ce déclenchement ne prétend pas
prouver à lui seul qu'une écriture disque était encore en cours exactement au
moment de `stop`; il vérifie la fermeture et la relecture dans cette fenêtre.

Le relais local applique indépendamment une probabilité de perte et un délai
uniforme entre zéro et `--delay-ms` dans les deux sens. Cela peut réordonner les
datagrammes; les octets ne sont jamais modifiés. Chaque client dispose de son
relais pour conserver la séparation des connexions. Le rapport conserve le
nombre de datagrammes perdus et exige des pertes effectives en mode perturbé.

## Commandes de sonde

Lancement : `primal-net-probe SERVER:PORT TOKEN_FILE`.
Une ligne JSON sur stdin donne une réponse avec le même `request_id` sur stdout.

```json
{"request_id":1,"op":"status"}
{"request_id":2,"op":"input","movement":[0,0,0],"flying":true,"yaw":0,"pitch":-1.5707963}
{"request_id":3,"op":"surface","x":0,"z":0}
{"request_id":4,"op":"ray"}
{"request_id":5,"op":"block","pos":[0,30,0]}
{"request_id":6,"op":"edit","pos":[0,30,0],"material":0,"revision":0}
{"request_id":7,"op":"quit"}
```

`input` reste actif et est envoyé toutes les 16 ms. `edit` répond avec le `seq`
envoyé, puis un événement asynchrone `outcome` donne le refus ou l'acceptation.
Sans `revision`, la sonde emploie la révision de son chunk répliqué. Les
événements `welcome`, `server_error` et `fatal` sont également asynchrones.
`surface` exige une colonne entièrement répliquée jusqu'au premier solide.
Le protocole utilise Hello/Ready/Bye sur Control, Edit sur Edits, Input sur Input;
la sonde reçoit tous les canaux et applique Chunk/Forget dans la réplique.
