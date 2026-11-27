# Rapport de production de la wyverne

Production réalisée sur PC 1 (`AMD Ryzen 7 7730U`, `AMD Radeon (TM) Graphics`, `15.38 GiB` RAM, Windows 11) sur la branche locale `art/wyvern` (worktree séparé).  
Les fichiers rust `crates/`, le manifeste provisoire [wyvern.json](../../assets/creatures/wyvern.json) et `assets/creatures/wyvern.glb` sont restés intacts.

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

### 3. Benchmark comparatif Bevy (`0`, `1`, `4`, `12` instances, `600` frames x `2` passes, 1280x720 Low)

Synthèse comparée sur la même machine entre `wyvern_art.json` (`16 418` tris, `7` textures) et le modèle provisoire `wyvern.json` (`6 048` tris, sans textures) ; détail dans [bevy_benchmark_summary.json](../previews/wyvern/art_c2/bevy_benchmark_summary.json) :

| Scénario | `wyvern_art` FPS médian | `wyvern_art` P99 | `wyvern_art` 1% low | `sched_us` | RSS max | Provisoire `wyvern.json` FPS / P99 |
| --- | --- | --- | --- | --- | --- | --- |
| `0` (forêt seule) | `197.0 FPS` | `7.02 ms` | `127.5 FPS` | `259.7 us` | `871.0 MB` | `197.0 FPS` / `7.58 ms` |
| `1-near-shadow-off` | `177.9 FPS` | `7.40 ms` | `128.4 FPS` | `467.0 us` | `902.8 MB` | `188.2 FPS` / `7.60 ms` |
| `1-near-shadow-on` | `168.1 FPS` | `10.22 ms` | `91.6 FPS` | `442.2 us` | `907.8 MB` | `187.5 FPS` / `7.41 ms` |
| `4-near-shadow-off` | `143.5 FPS` | `8.85 ms` | `104.3 FPS` | `629.2 us` | `903.8 MB` | `180.7 FPS` / `7.86 ms` |
| `4-near-shadow-on` | `135.6 FPS` | `9.77 ms` | `92.7 FPS` | `635.6 us` | `906.5 MB` | `183.7 FPS` / `7.66 ms` |
| `12-near-shadow-off` | `116.0 FPS` | `10.78 ms` | `86.1 FPS` | `728.9 us` | `893.4 MB` | `178.9 FPS` / `8.23 ms` |
| `12-near-shadow-on` | `104.9 FPS` | `11.50 ms` | `78.4 FPS` | `723.8 us` | `897.6 MB` | `176.6 FPS` / `7.58 ms` |
| `12-far-shadow-on` | `142.2 FPS` | `9.19 ms` | `99.5 FPS` | `697.9 us` | `895.1 MB` | — |
| `12-offscreen-shadow-on` | `163.8 FPS` | `8.98 ms` | `104.0 FPS` | `649.0 us` | `903.8 MB` | — |

Profilage GPU (`profile`, `300` frames) sur `wyvern_art.json` : à `12-near-shadow-on` (`197 016` tris rendus), `main_opaque_pass_3d/elapsed_gpu` passe de `2.61 ms` (`0`) à `6.15 ms` (`+3.54 ms`, dû aux `16 418` tris x `12` et normal maps) et `per_view_shadow_pass/elapsed_gpu` de `0.38 ms` à `0.89 ms`, tout en maintenant `> 100 FPS` sur GPU intégré.

### 4. Contrôle Git / LFS & Limites constatées

- Contrôles `tools/check-art-branch.py` et `git lfs fsck` à exécuter juste après le commit ART-C2 final.
- **Limites visibles honnêtement observées** :
  - Un minuscule interstice diagonal d'environ `1 px` subsiste en vue orthogonale stricte de liaison ([c2_bind_front.png](../previews/wyvern/art_c2/c2_bind_front.png)) entre le longeron externe discret (`step = 0.048 m`) et l'arête distale `wd -> tip` sur l'extrémité externe de l'aile (quasi imperceptible en poses d'animation et en vue jeu Bevy).
  - La sonde optionnelle `probe` (`offscreen-return`, `3600` frames) et le test de crossfade multi-secondes en client interactif continu restent gérés côté runtime Bevy (vérifiés statiquement par l'avance de boucle, la cohérence d'horloge `100 fps` et `render_transition_sequence_video`).
  - La branche `art/wyvern` reste locale (le push non interactif `origin art/wyvern` a échoué faute d'identifiants GitHub configurés sur ce poste ; sauvegarde intégrale + bundle maintenus hors worktree).
  - `art_pipeline/QUEUE.json` reste volontairement à `IN_PROGRESS` jusqu'à décision utilisateur de passage à `READY_FOR_INTEGRATION`.
