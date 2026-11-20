# Limites de cette version

- La vallée entière tient en mémoire. Ses frontières arrêtent le joueur et le terrain est généré au démarrage. Le streaming et les téléportations longue distance restent à construire.
- La géométrie statique est stockée dans des maillages CPU soumis à Macroquad. Le HUD indique la durée d'une frame complète. Le temps GPU isolé, la VRAM et le coût précis des uploads nécessitent encore des instruments dédiés.
- Une édition reconstruit les chunks touchés et synchronise la sauvegarde sur le fil du jeu. Des séries de modifications peuvent produire des saccades. Une écriture échouée est signalée et F5 permet de réessayer avant de fermer.
- La sauvegarde concerne le terrain. Un fichier endommagé est refusé. Une copie de secours historique et une récupération après perte électrique réelle restent à valider.
- Le contrôle du joueur utilise une boîte alignée sur les axes. Les pentes lisses, montures, plateformes mobiles et entités dynamiques sont des travaux ultérieurs.
- Le jeu se joue en solo. Le serveur dédié, le réseau, la survie, les créatures et les progressions sont décrits dans la feuille de route.
