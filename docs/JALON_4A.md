# Jalon 4A — Wyverne serveur

## Plan de livraison établi depuis main 4344026

1. Simuler à cadence fixe un groupe borné de wyvernes dans le noyau Rust. Reconstruire la démonstration au démarrage et mesurer les frontières des six états.
2. Étendre le protocole v1 par négociation. Les champs et discriminants des messages existants restent compatibles. Chaque snapshot de créatures décrit l’ensemble visible au tick donné. La perte d’un paquet est réparée par le suivant.
3. Associer chaque identifiant réseau à une instance Bevy. Interpoler dans le temps serveur et convertir en origine locale au moment de dessiner. Vérifier arrivées tardives, pertes, sortie de zone et reconnexions.
4. Tester le serveur avec deux clients Bevy et Macroquad. Mesurer séparément 0, 1, 4 et 12 créatures sur le matériel détecté, puis vérifier les tests historiques, la compilation et Clippy.

La commande artistique `wyvern` reste indépendante sur PC 2. Les six clips existants couvrent le besoin du jalon.
