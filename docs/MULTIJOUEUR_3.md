# Client Bevy multijoueur

## Lancer les deux clients sous Windows

Extraire entièrement le ZIP. Ouvrir `Serveur.cmd`, puis exécuter ces commandes dans deux terminaux ouverts dans le dossier extrait. Les identités sont locales et distinctes. Le fichier de configuration fourni écoute sur le port UDP 47620.

```powershell
.\PrimalVoxel.exe --connect 127.0.0.1:47620 --identity joueur-macro.identity
.\Jouer-Bevy.cmd --connect 127.0.0.1:47620 --identity joueur-bevy.identity
```

Un autre PC du LAN peut utiliser l’adresse du serveur. Dans Bevy, cliquer dans la fenêtre pour capturer la souris. WASD physiques déplacent le joueur, Espace saute, Maj accélère et F active le vol. En vol, Espace monte et Ctrl gauche descend. Les touches 1 à 7 sélectionnent le matériau, le clic gauche détruit et le clic droit pose le bloc visé. Échap libère la souris. R reconnecte le client Bevy avec la même identité. Un arrêt du transport déclenche une nouvelle tentative après une seconde. `Jouer-Bevy.cmd` sans argument ouvre la caméra libre solo.

## Autorité et rendu

Le serveur garde la physique, les collisions, la portée, les révisions et les sauvegardes régionales du jalon 2. Les exécutables graphiques utilisent la même version du protocole. Le serveur ne reçoit aucun indicateur de renderer.

Le transport dans `primal-net/src/link.rs` échantillonne les dernières commandes dans un thread dédié. Le saut reste mémorisé jusqu’au prochain envoi. La cadence par défaut est de 60 Hz, y compris si la fenêtre rend plus lentement. Le protocole historique n’annonce pas la cadence du serveur. Pour une configuration personnalisée, utiliser `--input-hz N` dans Bevy et `--tick-rate N` dans Macroquad. Le benchmark vérifie le débit effectif de chacun.

Chaque client possède une `Replica` du monde. Une édition se calcule depuis la position autoritaire de l’œil, le raycast partagé et la révision connue du chunk. Le serveur accepte ou refuse l’intention. Les snapshots périmés sont ignorés et le worker de meshing revalide sa révision avant de publier le résultat. Jusqu’à quatre chunks sont en cours de maillage. Le serveur et le stockage restent indépendants de l’API graphique. Bevy transforme les positions de joueurs et de chunks dans le repère de la caméra avant le rendu.

Les avatars Bevy sont des volumes simples. Les positions réseau existantes sont au format f32 et la commande administrative du serveur est bornée à deux millions de blocs. La démonstration du rebasage à ±1,5 milliard de blocs concerne la caméra solo. Le repère graphique ne restaure pas les décimales déjà perdues sur le réseau. Le client Bevy affiche actuellement les poses reçues à la cadence des snapshots. Macroquad interpole ses avatars distants.

## Validation

`tools/test-mixed-renderers.py BINAIRES DOSSIER_NEUF` lance réellement le serveur et deux fenêtres. Le mode de test local `--test-input` dirige le regard et les clics via l’entrée standard. Les deux clients empruntent le même raycast et le même chemin d’édition que leurs clics souris. Le contrôle attend une révision identique, le bon matériau, un chunk dessiné et une file de maillage vide sur les deux répliques. Un bloc a été détruit puis remplacé par chaque renderer. Les deux identités ont retrouvé le même bloc après reconnexions et après redémarrage du serveur. Les deux captures finales montrent l’avatar du pair. Les compteurs de pixels sont vérifiés au centre de chaque image. [Acceptation](../benchmarks/results/3b/acceptance.json) · [Vue Bevy](../benchmarks/results/3b/bevy-3.png) · [Vue Macroquad](../benchmarks/results/3b/macro-3.png)

Le test a aussi été rejoué avec les exécutables du ZIP extrait dans un dossier contenant un espace. Le lanceur Bevy a produit son rapport sur l’adaptateur Vulkan de la Radeon. Les 1 350 empreintes du manifeste ont été relues dans l’archive. [Preuve de livraison et SHA-256 du ZIP](../benchmarks/results/3b/portable.json)

## Interfaces pour le prochain jalon

| Fonction envisagée | Point d’intégration | Invariant à vérifier |
|---|---|---|
| Preset serveur de forêt | Ajouter à la création du monde un identifiant de preset et sa version, persistés avec la seed. Le générateur historique reste le défaut des mondes existants. Pour les snapshots différentiels, transmettre le preset au client avant toute reconstruction ou transmettre des blocs complets en cas de différence. | Les empreintes de la vallée et la lecture des sauvegardes v1/régionales doivent rester identiques. Rejeter explicitement une configuration incompatible. |
| Occlusion ambiante optionnelle | Étendre le snapshot et son halo aux voisins nécessaires, y compris diagonaux. Ajouter les quatre niveaux d’occlusion à la clé de fusion du masque greedy. Définir le choix de la diagonale et l’attribut de sommet côté rendu. Invalider les chunks adjacents touchés par une édition. | Mode désactivé identique au greedy actuel. Tester coins, bords et raccords sans fusion entre valeurs incompatibles. |
| Végétation décorative | Dériver les instances côté client de la seed, du preset et des coordonnées entières. Recalculer les instances concernées lors d’une révision ou d’un retrait de chunk. Les points de support viennent des blocs autoritaires. | Deux clients produisent les mêmes instances après reconnexion. Aucune instance ne participe à la collision, au raycast de blocs ou à la sauvegarde des éditions. |
| Créatures glTF/GLB | Associer un identifiant d’espèce serveur à une ressource visuelle locale. Définir les clips sur place, les noms de matériaux et leur correspondance dans un manifeste d’asset. Le serveur réplique pose et état, tandis que la racine de la scène graphique suit l’acteur dans le repère local. | Un asset manquant conserve un rendu de remplacement. Tester les clips sans déplacement de racine et signaler les noms de matériaux absents. |

Ces quatre interfaces sont des propositions pour le jalon suivant. Les mesures utilisent uniquement le contenu historique.
