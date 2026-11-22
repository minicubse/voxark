# Modèle du wyvern

Le [modèle original](../assets/creatures/wyvern.glb) est un bipède à longue queue. Ses ailes servent de membres antérieurs, avec un pouce griffu, des rayons visibles et des membranes festonnées. Les arcades protègent des yeux ambrés à pupille verticale. La corne droite ébréchée, une cicatrice sur la joue droite et la courbure de la queue rendent la silhouette asymétrique. Les écailles vert sombre et les membranes cuivrées sont colorées par sommet.

## Contrat d’export

- GLB 2.0 autonome, en mètres. Y monte, +Z pointe vers le museau et l’origine reste au sol sous le tronc.
- La scène par défaut nommée **Wyvern** contient le maillage skinné et le squelette. Le squelette articule les pattes postérieures, le tronc, le cou, la tête, la mâchoire, la queue, les épaules, les coudes, les poignets et les doigts. Les membranes combinent jusqu’à quatre influences par sommet.
- Les quatre matériaux PBR s’appellent **Scales**, **Horns**, **Eyes** et **Membranes**. Les membranes sont visibles des deux côtés. Les couleurs par sommet transportent les motifs et le matériau permet une teinte par variante.
- La racine du squelette conserve sa position pendant les animations. Le vol et la marche reçoivent leur déplacement et leur orientation du client. Le tronc bouge verticalement dans les clips.

| Clip | Durée | Lecture |
| --- | ---: | --- |
| IdleGround | 3,20 s | Boucle, ailes repliées et respiration |
| Walk | 1,04 s | Boucle, foulée bipède |
| Takeoff | 1,05 s | Transition, impulsion et déploiement des ailes |
| Flap | 1,02 s | Boucle, battement complet avec pli des poignets |
| Glide | 3,20 s | Boucle, ailes déployées et corrections d’assiette |
| Land | 1,10 s | Transition, freinage et repli des ailes |

## Reproduction et contrôles

Depuis la racine du dépôt, avec Python 3.10 ou ultérieur et sa bibliothèque standard.

~~~powershell
python tools/generate-wyvern.py
python tools/validate-wyvern.py
~~~

Le second script relit le binaire et contrôle les dimensions, les indices, les normales, l’arbre du squelette, les matrices inverses de liaison, les poids, les matériaux et les six clips. Le validateur Khronos glTF en version 2.0.0-dev.3.10 a relevé zéro erreur et zéro avertissement. Des vues des poses skinnées au repos, en marche, en battement et en plané ont été vérifiées.

Le binaire généré pèse **323 300 octets** et contient **3 403 sommets**, **6 048 triangles**, **23 articulations** et **4 primitives**. En pose de liaison, son envergure est de **7,31 m**, sa hauteur de **2,82 m** et sa longueur de **6,38 m**. Au repos, ses ailes repliées occupent environ **3,53 m** en largeur.

SHA-256 du fichier reproduit à l’identique avec Python 3.12.10.

~~~text
7b3fe3e3c8bb4fd4a50d632c7d6eea66239f4e32c177290c6db4984dffcc95f4
~~~
