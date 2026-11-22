# Wyverne voxel artistique

## Modèle de référence et fichiers à livrer

Comparer la silhouette et les poses avec le [modèle provisoire](../../assets/creatures/wyvern.glb) et sa [fiche technique](../../docs/WYVERN_ASSET.md). Les noms et champs viennent du [manifeste provisoire](../../assets/creatures/wyvern.json) et du [chargeur Bevy](../../crates/client-bevy/src/creature_asset.rs).

Livrer le manifeste indépendant `assets/creatures/wyvern_art.json`, la source Blender compressée `assets/creatures/wyvern_art/wyvern.blend`, le GLB autonome `assets/creatures/wyvern_art/wyvern.glb` et les textures PNG externes dans `assets/creatures/wyvern_art/textures/`. Placer les vues d’inspection dans `art_pipeline/previews/wyvern/` et renseigner `art_pipeline/reports/wyvern.md`. La mise en service par défaut relève de PC 1.

## Silhouette et gabarit

Créer une silhouette organique en voxels détaillés, avec deux pattes arrière et deux ailes servant de membres antérieurs. Articuler cou, tête et mâchoire. Détailler cornes, yeux, griffes, écailles, plaques, queue segmentée et membranes fixées aux doigts des ailes. Donner de la profondeur aux volumes principaux et faire varier le relief par zone. Préserver une asymétrie lisible dans la tête ou la queue. Regrouper le volume visible en maillages skinnés. Les grands plans à facettes, les arêtes et le relief doivent rester lisibles en jeu à distance moyenne.

| Mesure en pose de liaison | Référence ou cible de travail |
| --- | --- |
| Longueur totale, du museau à la queue | 6,38 m, mesurés dans le GLB de référence sur Z |
| Hauteur depuis le sol | 2,82 m, mesurés sur Y dans le GLB |
| Envergure, ailes déployées | 7,31 m, mesurés sur X dans le GLB. Le repos ailes repliées occupe environ 3,53 m de large |
| Corps, longueur × largeur × hauteur | Environ 1,82 × 1,28 × 1,12 m pour le tronc sans membres, cou, queue ni plaques dorsales, estimation de travail d’après les sections du générateur provisoire |
| Cou articulé | Environ 1,29 m, mesurés le long des segments `NeckBase` → `NeckTip` → `Head` du rig de référence |
| Queue | Environ 2,91 m projetés en Z entre la base du rig `TailBase` et l’extrémité du maillage de référence |
| Pivot | Origine `(0, 0, 0)` au sol sous le tronc, pieds au contact du sol en pose neutre |
| Échelle | 1 unité Blender = 1 m. `scale` du manifeste à `1.0` |

## Maillage, textures et repères

- Viser 10 000 à 18 000 triangles pour l’ensemble du LOD0 exporté, avec un plafond strict de 18 000. Retirer les surfaces cachées inutiles et réutiliser les matériaux et maillages adaptés. Déclarer uniquement les LOD effectivement livrés dans le rapport.
- Viser 1024 × 1024 pixels par texture, avec un plafond de 2048 × 2048. Conserver les PNG de production à côté du `.blend` sous `textures/`, avec des références relatives. Des textures normales et d’occlusion ambiante peuvent être calculées sur la RTX 3060 Ti. Le GLB incorpore les textures utilisées.
- Limiter chaque sommet à quatre influences normalisées. Vérifier le déploiement des membranes, l’appui des pieds, la flexion du cou, de la mâchoire et de la queue. Vérifier les matrices inverses de liaison et l’absence de déformation au repos dans le GLB réimporté.
- Dans Blender, `+Z` monte, `-Y` pointe vers le museau et `+X` est l’axe latéral. Utiliser une scène métrique `Wyvern`, facteur d’échelle `1.0`, et exporter avec `export_yup=True`.
- Dans le GLB, `+Y` monte et `+Z` pointe vers le museau. La conversion est `(x, y, z) → (x, z, -y)`. Sur une copie de travail, exporter puis réimporter un repère asymétrique vers le museau et sur le côté `+X`. Par exemple, le point Blender `(2, -3, 1)` doit être à `(2, 1, 3)` dans le GLB et revenir à `(2, -3, 1)` à la réimportation dans Blender. Consigner le résultat et retirer le repère de la scène de production.

## Animations

Chaque Action Blender porte le nom exact de la colonne GLB et est associée au rig d’export. Placer la racine au pivot fixe. Le tronc et les membres peuvent se déplacer localement, tandis que le jeu contrôle le déplacement mondial. Fermer la pose à la couture des boucles. Vérifier les transitions dans le client.

| Clé de `clips` | Nom GLB | Lecture et durée de référence |
| --- | --- | --- |
| `IdleGround` | `IdleGround` | Boucle, respiration et ailes repliées, 3,20 s |
| `Walk` | `Walk` | Boucle, foulée bipède, 1,04 s |
| `Takeoff` | `Takeoff` | Une lecture, impulsion et déploiement des ailes, 1,05 s |
| `Flap` | `Flap` | Boucle, battement et pli des poignets, 1,02 s |
| `Glide` | `Glide` | Boucle, ailes étendues et petites corrections d’assiette, 3,20 s |
| `Land` | `Land` | Une lecture, freinage et repli des ailes, 1,10 s |

## Matériaux et variantes

Utiliser quatre matériaux PBR nommés `Scales`, `Horns`, `Eyes` et `Membranes`. Les quatre clés de `slots` reprennent ces mêmes noms et pointent chacune vers le matériau homonyme. Laisser les membranes visibles des deux côtés (`doubleSided: true` dans le GLB) et contrôler l’éclairage et les ombres en vue du dessus et du dessous. Préparer les textures et couleurs de sommet pour que la teinte par palette laisse ressortir les motifs et les volumes.

Fournir `Jade`, `Braise` et `Obsidienne` dans `palettes`, avec une couleur sRGB `[r, g, b]` entre `0` et `1` pour chaque slot. Les valeurs du manifeste provisoire sont la référence initiale. Contrôler les trois variantes dans Bevy et inclure des vues comparables dans le rapport.

## Manifeste Bevy

Construire `assets/creatures/wyvern_art.json` lors de la production. `model` est relatif à la racine des assets (`assets`), et les noms de scène, de clips et de matériaux sont sensibles à la casse. Le chargeur refuse les champs inconnus.

| Champ | Valeur pour la livraison |
| --- | --- |
| `schema_version` | `1` |
| `model` | `creatures/wyvern_art/wyvern.glb` |
| `scene` | `Wyvern`, scène par défaut du GLB |
| `source_forward` | `+Z` |
| `up_axis` | `+Y` |
| `units` | `meters` |
| `scale` | `1.0` |
| `import_correction` | `bevy_gltf_scene_rotation` |
| `pivot` | `ground` |
| `root_motion` | `in_place` |
| `clips` | Les six correspondances exactes du tableau des animations |
| `slots` | `Scales`, `Horns`, `Eyes`, `Membranes` vers les matériaux homonymes |
| `palettes` | `Jade`, `Braise`, `Obsidienne` avec les quatre slots par variante |
