# Forêt moderne

Dans le ZIP Windows extrait, ouvrez **Jouer-Foret.cmd**. Le lanceur démarre le serveur sur le port 47621 et ouvre Bevy dans la clairière. Le premier clic capture la souris. Orientez-vous à la souris, marchez avec les touches physiques WASD (ZQSD sur AZERTY), utilisez Espace pour sauter et Maj pour courir. F active le vol. Les clics gauche et droit retirent et posent un bloc à portée du viseur. Les touches 1 à 7 sélectionnent le matériau. Fermez la fenêtre du client pour enregistrer et arrêter le serveur local.

Pour accueillir d’autres joueurs, démarrez **Serveur-Foret.cmd**. Bevy rejoint le serveur avec `Jouer-Bevy.cmd --connect ADRESSE:47621`. Le client Macroquad propose **Rejoindre** dans son menu. Chaque joueur utilise sa propre identité. Les commandes de console, la sauvegarde et la reprise sont décrites dans le [guide multijoueur](MULTIJOUEUR_2.md).

**Visiter-Foret.cmd** ouvre une caméra libre qui permet aussi de retrouver les points de vue des captures. Cette visite génère un monde temporaire. La partie jouable utilise `worlds/foret.pvx.regions`. La vallée conserve ses lanceurs et son monde. Pour sauvegarder une partie sur un autre disque, arrêtez le serveur puis copiez le dossier régional complet et les identités nécessaires.

## Génération et éditions

Le serveur lit `"preset": "forest-modern"` dans sa configuration JSON ou accepte `--preset forest-modern`. Une nouvelle région de monde utilise le générateur enregistré dans son manifeste. À la réouverture, le serveur retrouve son preset même si l’option est omise. Un preset explicitement incompatible déclenche une erreur. Les sauvegardes historiques v2 restent lisibles. Les nouvelles métadonnées de forêt v3 portent le preset et sa version de génération.

Le relief et les arbres sont calculés à partir des coordonnées mondiales et de la seed. À partir de la clairière centrale, le versant oriental découvre une barre rocheuse. Le générateur construit résineux étagés, feuillus à charpente étalée et arbres à couronne étroite. L’âge module le diamètre, la hauteur et les racines. Les blocs de bois mort, de roche, de terrain et de feuillage se détruisent individuellement. Les collisions et les décisions d’édition restent celles du serveur. Les messages réseau gardent leur format. En forêt, le serveur transmet les chunks complets et ordonne le message d’accueil sur leur canal fiable.

Bevy regroupe la micro-végétation en deux maillages par chunk de surface, un proche et un lointain clairsemé. LOD et position suivent le chunk, y compris après un changement d’origine. Le hachage de la seed et de la cellule support conserve les mêmes plantes après un rechargement. Le retrait du support ou sa substitution par de la roche élimine les plantes correspondantes au prochain maillage. Les deux niveaux de détail utilisent les mêmes supports. Ces plantes reçoivent l’éclairage et ne projettent pas d’ombre. Le générateur réserve la place de la géométrie structurelle avant les plantes, ce qui borne aussi les maillages de chunks artificiels à plusieurs étages cultivés.

Le shader utilise des coordonnées locales et une phase de chunk calculée en entier. La couleur varie à l’intérieur des grands quads. L’AO, indépendante, utilise les 26 voisins possibles, conserve les bords non uniformes et choisit la diagonale suivant les quatre niveaux d’occlusion. Les tests couvrent l’invalidation diagonale et les orientations du maillage opaque, y compris dans une cavité.

## Réglages du client Bevy

| Option | Valeur Low | Effet |
|---|---|---|
| `--render-radius` | 4 | Rayon du rendu en chunks de 16 blocs |
| `--density` | 0.58 | Fraction maximale de la distribution botanique, de 0 à 1 |
| `--vegetation-distance` | 46 | Portée de la végétation, en blocs, avec transition par chunk |
| `--lighting` / `--fog` | on / on | Éclairage et brume indépendants |
| `--shadows` | 1 | Zéro, une ou deux cascades |
| `--shadow-size` / `--shadow-distance` | 1024 / 44 | Résolution de la carte et portée des ombres |
| `--ao` | on | Occlusion par sommet |
| `--palette` / `--variation` | on / on | Palette naturelle et grain procédural |
| `--culling` | on | Élimination des faces arrière opaques |
| `--tone` | tony | TonyMcMapface, AgX ou aucun (`tony`, `agx`, `none`) |

Exemple de visite sans ombres dynamiques avec végétation allégée : `Visiter-Foret.cmd --shadows 0 --density 0.35`. Ces options peuvent aussi suivre `--connect` dans la commande de connexion au serveur. Le rendu emploie les capacités natives de Bevy 0.20.1. Les fonctionnalités de tables de tonemapping et de décompression Zstandard permettent d’embarquer les tables du moteur dans le client. La végétation, les textures procédurales et le ciel sont produits par le projet. Les notices de la distribution indiquent les versions et la provenance des tables de couleur.

[Captures, limites visuelles et mesures de la Radeon](PERFORMANCE_3C.md)
