# Rapport de production de la wyverne

Production réalisée sur PC 1 (`AMD Ryzen 7 7730U`, `AMD Radeon (TM) Graphics`, `15.38 GiB` RAM, Windows 11) sur la branche locale `art/wyvern` (worktree séparé).  
Les fichiers rust `crates/`, le manifeste provisoire [wyvern.json](../../assets/creatures/wyvern.json) et `assets/creatures/wyvern.glb` sont restés intacts.

## Provenance et droits d'utilisation

### Origine technique de la livraison

La wyverne de cette PR a été construite pour Primal Voxel par génération procédurale dans Blender et Python, avec l’assistance de Gemini dans Google Antigravity. PC 1 a vérifié la trace locale de la session de production : elle associe les métadonnées de génération Gemini aux opérations sur les quatre scripts ci-dessous, aux commits `95d7621` et `5b99665` et à l’empreinte du GLB livré. Le nom d’auteur Git seul ne permettrait pas cette attribution. Les échanges privés de la session ne font pas partie de la livraison.

| Fichier | Origine et rôle vérifiés |
|---|---|
| `prototype.py` | Prototype de silhouette construit dans une scène vide avec les primitives Blender et les fonctions de maillage du projet. Première version suivie dans `d00a40c`. |
| `rig_skin_c1.py` | Étape technique du même commit. Le script construit le maillage unifié, l’armature et les poids. Son import glTF est un contrôle de réimportation du GLB qu’il vient d’exporter. |
| `build_art_c2.py` | Générateur livré dans `95d7621`. Il part d’une scène vide, construit le maillage, les matériaux, les 23 os et les six actions, puis enregistre `wyvern.blend`. |
| `generate_textures.py` | Générateur Python du même commit. Il calcule les motifs d’écailles, de cornes, de membranes et d’yeux et encode les pixels en PNG avec la bibliothèque standard. Ses entrées sont des paramètres numériques. |
| `wyvern.glb` | Export de la scène `Wyvern` du .blend par le script du projet `tools/blender/wyvern/build.py`, déjà présent sur la branche de base. Ce script appelle l’exporteur glTF livré avec Blender. |

**Contrôle indépendant sur PC 1.** Depuis une scène vide, les fonctions de construction de C2 ont reproduit les coordonnées des 10 070 sommets, les indices des 16 418 faces et leur affectation aux matériaux du .blend livré. L’ouverture de ce .blend avec l’exécution automatique désactivée a révélé zéro bibliothèque Blender liée. Ses seuls chemins de ressources sont les sept PNG relatifs sous `//textures/`. Le générateur a reproduit chacun de ces sept fichiers à l’octet près dans un dossier temporaire. Les sept images embarquées dans le GLB correspondent pixel pour pixel aux PNG et le GLB ne référence aucun tampon ni aucune image par URI externe.

Enfin, un nouvel export du .blend livré avec Blender 4.5.0 et l’exporteur glTF 4.5.47 a reproduit **exactement** le GLB dont le SHA-256 est `cd5564529fd055ab1be3663eb9d125545f9605e653201c6bb09eab6faf400be6`. Ces contrôles établissent la filière de la géométrie et des textures livrées sans import de modèle Meshy ni incorporation d’une ressource graphique tierce identifiée. Les dix empreintes publiées pour le .blend, les deux GLB et les sept PNG sont celles des fichiers vérifiés ci-dessous.

### Outils et conditions d’utilisation

- **Blender et son API Python** (`bpy`, `bmesh`, `mathutils`) servent à créer le maillage, le rig et les actions. La GPL de Blender régit le logiciel. Son utilisation pour produire une œuvre graphique ne place pas à elle seule cette œuvre sous GPL. [Licence Blender](https://www.blender.org/about/license/) · [FAQ GNU sur les sorties d’un programme](https://www.gnu.org/licenses/gpl-faq.html#GPLOutput)
- **Khronos glTF-Blender-IO 4.5.47** assure l’export. Le module installé déclare `Apache-2.0`. Une redistribution du module doit respecter sa licence et ses notices. Le GLB contient les données de la scène exportée. [Licence de l’exporteur Khronos](https://github.com/KhronosGroup/glTF-Blender-IO/blob/main/LICENSE.txt)
- **Python 3.11 et zlib** fournissent les fonctions mathématiques, l’encodage binaire et la compression des PNG. Les conditions PSF et les notices des composants, notamment celle de zlib, s’appliquent à la redistribution de ces logiciels. [Licences Python et notice zlib](https://docs.python.org/3.11/license.html#zlib)
- **Gemini dans Google Antigravity** a assisté la production des scripts. L’usage du service relève des conditions Google et Antigravity, ou du contrat administrateur pour les offres professionnelles concernées. La trace technique ne précise pas l’offre contractuelle du compte. [Conditions Antigravity](https://antigravity.google/terms) · [Conditions Google](https://policies.google.com/terms?hl=fr)

### Licence du projet

Ces ressources sont livrées pour l’intégration à Primal Voxel. Au commit artistique examiné, le dépôt ne publie aucune licence explicite de réutilisation des modèles, textures ou quatre scripts de production. Le choix d’une licence pour leur diffusion appartient aux titulaires des droits. Cette section documente les outils et les apports identifiés. Elle n’accorde pas de licence à des tiers et ne garantit aucune exclusivité juridique.

## Fichiers et reproduction

- **Blender** : `Blender 4.5.0` LTS officiel (`hash 8cb6b388974a`, Python `3.11.11`, `bpy 4.5.0`, `BLENDER_EEVEE_NEXT`), installé localement hors `%TEMP%`.
- **Scripts de génération et d'export** :
  - Générateur de textures : `tools/blender/wyvern/generate_textures.py`
  - Générateur scène/rig/animations/rendus : `tools/blender/wyvern/build_art_c2.py`
  - Exporteur officiel utilisé tel quel : `tools/blender/wyvern/build.py`
- **Source Blender** : `assets/creatures/wyvern_art/wyvern.blend` (`731 825` octets, suivi Git LFS)
  - SHA-256 : `e75738401d0011d479227cab161d047e2b74a6c5bf50fa7eb4d1e7ac842049ad`
- **GLB livrable** : `assets/creatures/wyvern_art/wyvern.glb` (`3 286 428` octets, suivi Git LFS)
  - SHA-256 : `cd5564529fd055ab1be3663eb9d125545f9605e653201c6bb09eab6faf400be6`
  - Artefact technique conservé d'ART-C1 : `assets/creatures/wyvern_art/wyvern_tech.glb` (`1 729 040` octets, SHA-256 `923da8a841739bb766d3221c38c21b8b98d93b10b15c9bc0128756537eb3119f`)
- **Manifeste artistique** : `assets/creatures/wyvern_art.json`
- **LOD livré** : `LOD0` unique (`16 418` triangles <= plafond `18 000`).

### Commandes de reproduction exactes

```powershell
# 1. Génération du .blend, des textures //textures/... et des rendus de contrôle
blender.exe --background --factory-startup --python-exit-code 1 --python tools/blender/wyvern/build_art_c2.py

# 2. Export officiel du GLB définitif
blender.exe --background --factory-startup --disable-autoexec --python-exit-code 1 --python tools/blender/wyvern/build.py
```

### Textures PNG (`//textures/...` embarquées dans `wyvern.glb`)

| Fichier | Dimensions | Taille | Rôle / Espace | SHA-256 |
| --- | --- | --- | --- | --- |
| `wyvern_scales_albedo.png` | `1024x1024` | `53 072 B` | Albedo neutre écailles (`sRGB`) | `95e397af431d66aab20fd756c2e1520232f930ec93d0c235074573c8bbc2323b` |
| `wyvern_scales_normal.png` | `1024x1024` | `153 549 B` | Normales écailles (`Non-Color`, +Y OpenGL) | `0544b273a3887d12764e5fbb9bb0ebc32acf39dd9c91340bd6ec06dae1986349` |
| `wyvern_horns_albedo.png` | `1024x1024` | `32 089 B` | Albedo neutre stries cornes (`sRGB`) | `8e3f571fb1792bc3981f53125875b90fed15e643e4b7ea3f40a7d7a6b37cb392` |
| `wyvern_horns_normal.png` | `1024x1024` | `67 005 B` | Normales stries cornes (`Non-Color`) | `3f8660d0be2fc4ab7148d51db6b9a5e1278c1a55ca749f3f088145b1f343231e` |
| `wyvern_membranes_albedo.png` | `1024x1024` | `34 980 B` | Veinures alaires neutres (`sRGB`) | `7d6fee1a55c1a2ddc76cd44e284e2000d30ae5b3dee68cfe192d894e1ff3224c` |
| `wyvern_membranes_normal.png` | `1024x1024` | `49 635 B` | Relief nervures membranes (`Non-Color`) | `727ace6705d5a0fcb977ea5ee32cc99ceb120fef6f3f961a5d6cc8e1cee83773` |
| `wyvern_eyes_albedo.png` | `512x512` | `4 580 B` | Pupille fendue + éclat (`sRGB`) | `c414570c8bbfd28ea7995651c56f79dc642721b09af6753a405ba75515de53d6` |

## Modèle et animation

- **Topologie & sommets** :
  - Maillage source `Wyvern_Mesh` (`wyvern.blend`) : `10 070` sommets partagés, `16 418` polygones triangulés (`49 254` boucles, `16 418` triangles).
  - GLB exporté `Wyvern` (`wyvern.glb`) : `1` instance de maillage, `4` primitives, **`16 418` triangles**, **`30 128` sommets exportés**. L'écart `10 070` -> `30 128` provient du dédoublement normal/UV/tangentes par face imposé par le rendu voxel flat-shaded (`NORMAL`, `TANGENT`, `TEXCOORD_0`, `COLOR_0`) et par les 4 primitives matériaux glTF.
  - Répartition par matériau : `Scales` (`25 287` v / `14 276` tris, `doubleSided` défaut), `Horns` (`2 577` v / `1 378` tris), `Eyes` (`16` v / `8` tris), `Membranes` (`2 248` v / `756` tris, `doubleSided: true`).
- **Armature & skinning** :
  - `Wyvern_Rig` : `23` articulations (`Root`, `Body`, `NeckBase`, `NeckTip`, `Head`, `Jaw`, `TailBase`, `TailMid`, `TailTip`, `Hip/Knee/Ankle.L/R`, `WingShoulder/Elbow/Wrist/Digit.L/R`), `1` skin avec `inverseBindMatrices`, maximum **`4` influences par sommet** normalisées à `1.0`.
  - Correction critique ART-C2 sur `wing_weight` : ajout de la garde de distance `best_df < d_main` pour éviter qu'un seuil sur `pt.y > ww.y + 0.12` ne capture la racine du bras (`WingShoulder`) dans `WingWrist` au repli ; suppression complète de la déformation sur le devant du poitrail.
- **Dimensions, pivot et repère (roundtrip `import_scene.gltf` Blender 4.5 en pose neutre)** :
  - Englobante : `min = [-3.648, -2.688, 0.0]`, `max = [3.648, 3.658, 2.832]` m
  - Dimensions : envergure **`7.296 m`** (cible `7.31 m`), longueur **`6.346 m`** (cible `6.38 m`), hauteur **`2.832 m`** (cible `2.82 m`), pivot exact au sol `Z_min = 0.000 m`.
  - Asymétrie volontaire (cicatrice joue droite `-X`, corne droite légèrement plus courte) conservée sans inversion droite/gauche après export/réimportation (`+Z` avant glTF, `+Y` haut glTF).
  - Ombres vérifiées sur les deux faces des membranes dans le client Bevy (`flap-front`, `flap-back`, `near-12-shadow-on`).

### Clips d'animation (100 fps exportés via `ACTIONS` sans mouvement horizontal de racine)

| Clip | Type | Durée exportée | Tracks | Z_min monde | Z_min membrane | Erreur bouclage quat / Z |
| --- | --- | --- | --- | --- | --- | --- |
| `IdleGround` | Boucle | `3.2000 s` | `69` | `0.0000 m` | `1.0166 m` | `1e-7` / `0.0 m` |
| `Walk` | Boucle | `1.0400 s` | `69` | `0.0000 m` | `1.0334 m` | `1e-7` / `0.0 m` |
| `Takeoff` | One-shot | `1.0500 s` | `69` | `0.0000 m` | `0.4624 m` | — |
| `Flap` | Boucle | `1.0200 s` | `69` | `+0.1414 m` | `0.7063 m` | `1e-7` / `0.0 m` |
| `Glide` | Boucle | `3.2000 s` | `69` | `+0.1792 m` | `1.3772 m` | `1e-7` / `0.0 m` |
| `Land` | One-shot | `1.1000 s` | `69` | `0.0000 m` | `0.3250 m` | — |

Transitions simulées (crossfade Bevy `220 ms`, fraction `0.5`) : sur les 8 paires (`IdleGround<->Walk`, `IdleGround->Takeoff`, `Takeoff->Flap`, `Flap<->Glide`, `Flap->Land`, `Land->IdleGround`), `Z_min` reste compris entre `-0.0041 m` et `+0.2756 m`, et `Z_min_membrane >= 1.0172 m`. En vol (`Flap`/`Glide`/`Takeoff`/`Land`), une élévation verticale modérée de `Body` maintient les serres et membranes hors du plan d'ombre au sol en prévisualisation sans ajouter de déplacement horizontal (`root_motion: in_place`).

### Palettes

- `Jade` : écailles `[0.11, 0.42, 0.32]`, cornes `[0.76, 0.67, 0.45]`, yeux `[1.0, 0.72, 0.2]`, membranes `[0.18, 0.65, 0.56]`
- `Braise` : écailles `[0.55, 0.14, 0.08]`, cornes `[0.34, 0.24, 0.18]`, yeux `[1.0, 0.8, 0.27]`, membranes `[0.92, 0.42, 0.15]`
- `Obsidienne` : écailles `[0.13, 0.17, 0.23]`, cornes `[0.56, 0.49, 0.6]`, yeux `[0.54, 0.83, 1.0]`, membranes `[0.16, 0.48, 0.62]`

Les textures PNG et couleurs sommet `COLOR_0` sont maintenues dans une plage de luminance neutre (`[0.54..1.0]`, hors orbite `0.20`) afin que Bevy applique `base_color` sans saturation écrasée ni dérive de teinte.

## Prévisualisations

### Captures Blender 4.5 (Eevee, échelle humaine `1.80 m`)

- Pose de liaison (face / dessus) : [c2_bind_front.png](../previews/wyvern/art_c2/c2_bind_front.png) · [c2_bind_top.png](../previews/wyvern/art_c2/c2_bind_top.png)
- Sol (`IdleGround` / `Walk`) : [c2_idle_00_front.png](../previews/wyvern/art_c2/c2_idle_00_front.png) · [c2_idle_00_profile.png](../previews/wyvern/art_c2/c2_idle_00_profile.png) · [c2_walk_25_profile.png](../previews/wyvern/art_c2/c2_walk_25_profile.png) · [c2_walk_75_profile.png](../previews/wyvern/art_c2/c2_walk_75_profile.png)
- Vol (`Takeoff` / `Flap` / `Glide` / `Land`) : [c2_takeoff_50_3q.png](../previews/wyvern/art_c2/c2_takeoff_50_3q.png) · [c2_flap_25_front.png](../previews/wyvern/art_c2/c2_flap_25_front.png) · [c2_flap_50_profile.png](../previews/wyvern/art_c2/c2_flap_50_profile.png) · [c2_flap_75_3q.png](../previews/wyvern/art_c2/c2_flap_75_3q.png) · [c2_glide_50_3q.png](../previews/wyvern/art_c2/c2_glide_50_3q.png) · [c2_land_50_profile.png](../previews/wyvern/art_c2/c2_land_50_profile.png)
- Trois palettes : [c2_palette_jade.png](../previews/wyvern/art_c2/c2_palette_jade.png) · [c2_palette_braise.png](../previews/wyvern/art_c2/c2_palette_braise.png) · [c2_palette_obsidienne.png](../previews/wyvern/art_c2/c2_palette_obsidienne.png)
- Transitions & séquence complète MP4 (`184` frames @ `25 fps`, crossfades `220 ms`) : [c2_trans_takeoff_to_flap.png](../previews/wyvern/art_c2/c2_trans_takeoff_to_flap.png) · [c2_trans_land_to_idleground.png](../previews/wyvern/art_c2/c2_trans_land_to_idleground.png) · [wyvern_sequence_preview.mp4](../previews/wyvern/art_c2/wyvern_sequence_preview.mp4)

### Captures réelles dans `PrimalVoxel-Bevy.exe` (Vulkan / Radeon)

- Sol & transitions : [bevy_idle-side.png](../previews/wyvern/art_c2/bevy/bevy_idle-side.png) · [bevy_walk-side.png](../previews/wyvern/art_c2/bevy/bevy_walk-side.png) · [bevy_takeoff-side.png](../previews/wyvern/art_c2/bevy/bevy_takeoff-side.png) · [bevy_land-side.png](../previews/wyvern/art_c2/bevy/bevy_land-side.png)
- Battement / vol (`doubleSided` face, dos, profil) : [bevy_flap-side-a.png](../previews/wyvern/art_c2/bevy/bevy_flap-side-a.png) · [bevy_flap-front.png](../previews/wyvern/art_c2/bevy/bevy_flap-front.png) · [bevy_flap-back.png](../previews/wyvern/art_c2/bevy/bevy_flap-back.png) · [bevy_glide-side.png](../previews/wyvern/art_c2/bevy/bevy_glide-side.png)
- Palettes & groupe (`4` / `12` instances avec ombres ON) : [bevy_near-4-palettes.png](../previews/wyvern/art_c2/bevy/bevy_near-4-palettes.png) · [bevy_near-12-shadow-on.png](../previews/wyvern/art_c2/bevy/bevy_near-12-shadow-on.png)

## Validation

### 1. Validateur statique (`validate-creature.py`)

```powershell
python tools/validate-creature.py --manifest assets/creatures/wyvern_art.json --asset-root assets --result art_pipeline/previews/wyvern/art_c2/validate_creature_report.json
```

- **Code de sortie** : `0` (`"valid": true`, `"errors": []`)
- **Résultat** : `16 418` tris (`<= 18 000`), `30 128` sommets, `1` mesh instance, `1` skin, `23` os max, `7` textures PNG embarquées (`6x 1024x1024`, `1x 512x512`), `4` slots (`Scales`, `Horns`, `Eyes`, `Membranes`), `6` clips exacts (`69` tracks chacun), aucune motion horizontale de `Root`/`Body`. Voir [validate_creature_report.json](../previews/wyvern/art_c2/validate_creature_report.json).

### 2. Import réel dans `PrimalVoxel-Bevy.exe` (`test-creature-import.py`)

- **Client utilisé** : binaire précompilé `PrimalVoxel-3D-Windows-x64/PrimalVoxel-Bevy.exe` (SHA-256 `375612d187f8fa516ace9f3f850a960bef6b800db5de1f9ca8a904795ef71472`), vérifié depuis l'archive `PrimalVoxel-3D-Windows-x64.zip` (`f8e42d08ec09f55c9534c60eeffb68c96e0d4bf50d7ee6202d121499470f6a83`), exécuté sur `AMD Radeon (TM) Graphics` (`Vulkan`).

```powershell
python tools/test-creature-import.py --client "...\PrimalVoxel-Bevy.exe" --manifest assets/creatures/wyvern_art.json --asset-root assets --result art_pipeline/previews/wyvern/art_c2/bevy_import_report.json
```

- **Code de sortie** : `0` (`"valid": true`)
- **Métriques** : `asset_load_ms = 79.41 ms`, `clips = 6`, `instances = 1`, `max_joints = 23`, `mesh_assets = 4`, `skinned_meshes = 4`, `skins = 1`, `slots = 4`, `palette_materials = 12`. Voir [bevy_import_report.json](../previews/wyvern/art_c2/bevy_import_report.json).

### 3. Benchmark comparatif Bevy & Sonde longue (`ART-C2` + `ART-C3`, Ryzen 7 7730U / Radeon Vulkan, Low `1280x720`)

#### Sonde longue `offscreen-return` (`3 600` images, `4` créatures proches, ombres ON)

```powershell
python tools/benchmark-creatures.py probe --client "...\PrimalVoxel-Bevy.exe" --asset-root assets --creature-manifest creatures/wyvern_art.json --frames 3600 --repeats 1 --output "...\bevy_probe_c3"
```

- **Code de sortie** : `0` (`probe-offscreen-return`)
- **Performances** : **`137.4 FPS` moyens**, **`85.5 FPS` (`1 % low`)**, **`10.42 ms` (`P99`)**, `animation_schedule_us` moyen `544.8 us`, RSS max `931.8 MiB` (`977 084 416 B`).
- **Horloge et animation après retour dans le champ** :
  - `before` (`0.673 s`) : `visible_meshes = 4`, `elapsed_s = 0.667 s`
  - `first_away` (`1.166 s`) -> `last_away` (`3.390 s`) : `visible_meshes = 0`, `elapsed_s` progresse continûment (`1.160 s -> 3.383 s`)
  - `returned` (`3.876 s`) : `visible_meshes = 4`, `elapsed_s = 3.870 s`
  - Articulations mesurées en rotation hors champ : `Body`, `Head`, `Jaw`, `TailTip`, `WingWrist.L`, `WingWrist.R`.

#### Mesures soutenues `600 f x 2` et prolongées `4 200 f x 3` (`wyvern_art.json` vs provisoire `wyvern.json`)

Série courte complète (`600` frames x `2` passes, détail dans [bevy_benchmark_summary.json](../previews/wyvern/art_c2/bevy_benchmark_summary.json)) et série prolongée (`4 200` frames x `3` passes alternées, détail dans [art_c3_validation_summary.json](../previews/wyvern/art_c2/art_c3_validation_summary.json)) :

| Scénario (`ombres ON`, `4 200 f x 3`) | `wyvern_art` FPS moyens | `wyvern_art` 1 % low | `wyvern_art` P99 | `sched_us` | Provisoire `wyvern.json` FPS / 1 % low / P99 |
| --- | --- | --- | --- | --- | --- |
| `0` (forêt seule) | `202.9 FPS` | `117.6 FPS` | `7.59 ms` | `281.0 us` | — (`197.0` sur `600 f`) |
| `1-near-shadow-on` | `184.0 FPS` | `120.1 FPS` | `7.54 ms` | `473.4 us` | — (`187.5` sur `600 f`) |
| `4-near-shadow-on` | `139.7 FPS` | `92.2 FPS` | `9.47 ms` | `602.6 us` | — (`183.7` sur `600 f`) |
| `12-near-shadow-on` | **`107.0 FPS`** | **`79.5 FPS`** | **`11.31 ms`** | `643.3 us` | `183.9 FPS` / `131.1 FPS` / `6.99 ms` |

Série complémentaire `600 f x 2` (`shadow-off`, `far`, `offscreen`) sur `wyvern_art.json` :
- `1-near-shadow-off` : `177.9 FPS` (`1% low 128.4`, `P99 7.40 ms`) · `1-near-shadow-on` : `168.1 FPS` (`1% low 91.6`, `P99 10.22 ms`)
- `4-near-shadow-off` : `143.5 FPS` (`1% low 104.3`, `P99 8.85 ms`) · `4-near-shadow-on` : `135.6 FPS` (`1% low 92.7`, `P99 9.77 ms`)
- `12-near-shadow-off` : `116.0 FPS` (`1% low 86.1`, `P99 10.78 ms`) · `12-near-shadow-on` : `104.9 FPS` (`1% low 78.4`, `P99 11.50 ms`)
- `12-far-shadow-on` : `142.2 FPS` (`1% low 99.5`, `P99 9.19 ms`) · `12-offscreen-shadow-on` : `163.8 FPS` (`1% low 104.0`, `P99 8.98 ms`)

#### Analyse causale de l'écart à `12-near-shadow-on` (`107.0 FPS` vs `183.9 FPS` provisoire)

Les mesures comparées `profile` (`300` frames) et `trace` (`wgpu`, `NO_COLOR=1`) permettent de dissocier précisément chaque facteur :

| Poste mesuré (`12-near-shadow-on`) | Provisoire `wyvern.json` | Artistique `wyvern_art.json` | Delta & explication |
| --- | --- | --- | --- |
| Géométrie / tris visibles (`12x`) | `72 576` tris (`6 048/u`) | `197 016` tris (`16 418/u`) | **`2.71x`** triangles |
| Sommets glTF & buffers GPU | `3 403` v/u (`254 080 B`) | `30 128` v/u (`2 749 772 B`) | **`8.85x` sommets**, **`10.82x` mémoire mesh** (`TANGENT`, `TEXCOORD_0`, split flat-voxel) |
| Skinning CPU (`23` os x `12` = `276`) | `534.8 us` (`4200f`) | `643.3 us` (`4200f`) | **`+108.5 us/frame` seulement** (`cpu_cores 2.35` vs `2.92`) : pas de goulot CPU |
| `early_mesh_preprocessing` GPU | `0.0155 ms` | `0.0163 ms` | `+0.0008 ms` (négligeable) |
| Matériaux & shading opaque (`main_opaque_pass_3d`) | `2.9619 ms` (`0` img) | **`6.1515 ms`** (`7` PNG + `3` norm) | **`+3.19 ms` (~85 % du coût GPU)** : transformation de `361k` sommets + PBR avec normal maps `1024x1024` + `Membranes` `doubleSided` |
| Pass d'ombres (`per_view_shadow_pass`) | `0.3568 ms` | **`0.8944 ms`** | **`+0.54 ms` (~14 % du coût GPU)** sur `197k` tris dans les cascades |
| Appels `wgpu` (`trace`, par intervalle) | `12` draw, `37` writes, `569 us` | `14` draw, `43` writes, `840 us` | Ordonnancement quasi inchangé (`2` submits) |

L'écart de `76.9 FPS` entre `wyvern_art` (`107.0 FPS` / `9.35 ms`) et le modèle provisoire (`183.9 FPS` / `5.44 ms`) provient donc essentiellement du pipeline fragment/vertex GPU dans `main_opaque_pass_3d` (texture+normal mapping + `8.85x` sommets splittés) et secondairement du rendu d'ombre (`+0.54 ms`), tout en conservant une marge confortable par rapport à la cible minimale d'acceptation (`>= 60 FPS` moyens : **`+78 %` de marge** en moyenne et **`79.5 FPS` au 1 % low**).

### 4. Dernière revue artistique, contrôle Git / LFS & Limites

- **Revue micro-interstice en bout d'aile (`ART-C3`)** : l'examen comparatif entre [c2_bind_front.png](../previews/wyvern/art_c2/c2_bind_front.png) et les vues Bevy en vol ([bevy_flap-front.png](../previews/wyvern/art_c2/bevy/bevy_flap-front.png), [bevy_glide-side.png](../previews/wyvern/art_c2/bevy/bevy_glide-side.png), [bevy_near-4-palettes.png](../previews/wyvern/art_c2/bevy/bevy_near-4-palettes.png)) confirme que le micro-jour diagonal (`~1 px`) entre longeron voxel discret (`0.048 m`) et membrane distale n'apparaît qu'en projection orthogonale stricte de liaison et est imperceptible en jeu et dans toutes les animations. Conformément à la règle ART-C3 (« ne pas modifier arbitrairement une géométrie validée pour un défaut imperceptible en jeu »), la topologie, les poids et le `.glb` validés sont conservés intacts.
- **Contrôles Git / LFS** : `tools/check-art-branch.py` et `git lfs fsck` validés après le commit ART-C2 (`95d7621`) et réexécutés sur le commit final `READY_FOR_INTEGRATION`.
- **Statut** : `wyvern` passé à **`READY_FOR_INTEGRATION`** dans `art_pipeline/QUEUE.json` ; aucune fusion sur `main` ni modification du modèle provisoire [wyvern.json](../../assets/creatures/wyvern.json) ou `assets/creatures/wyvern.glb`.
