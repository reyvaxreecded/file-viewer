# Architecture

## Choix retenus

Workspace Cargo, Rust stable, API JSON Axum/Tokio et client Leptos CSR. L’interface est une application WebAssembly sans React ni Vue. Un module JavaScript local gère les API natives File/Blob, les sélecteurs, les dépôts de dossiers et le réseau des uploads. Les octets des gros fichiers ne traversent pas la mémoire WebAssembly.

Versions résolues dans `Cargo.lock` : Axum 0.8.9, Leptos 0.8.23, russh 0.65.1, russh-sftp 3.0.1, rusqlite 0.37.0. Les versions stables ont été consultées dans les documentations officielles le 10 novembre 2026. La compilation réelle a été vérifiée avec Rust 1.92.0 sur macOS arm64 et wasm32. Leptos 0.9 en préversion n’a pas été sélectionné. Les dépendances transitives de russh incluent ssh-key en release candidate ; elles sont verrouillées et doivent rester couvertes par l’audit de dépendances.

- [Axum](https://docs.rs/axum/0.8.9/axum/)
- [Leptos](https://docs.rs/leptos/0.8.23/leptos/)
- [Vérification des clés russh](https://docs.rs/russh/0.65.1/russh/client/trait.Handler.html)
- [Protocole russh-sftp](https://docs.rs/russh-sftp/3.0.1/russh_sftp/client/struct.RawSftpSession.html)

La chaîne SSH/SFTP est asynchrone. SQLite est isolé dans `spawn_blocking`, avec un verrou limité à la transaction locale. Le backend n’accède jamais au répertoire partagé via `std::fs` ; le fichier de mot de passe, les assets et SQLite constituent ses seuls fichiers locaux. Le profil systemd rend le répertoire partagé inaccessible au compte de service HTTP.

## Connexions et mémoire

Pool de six connexions, quatre transferts maximum par défaut. Une connexion appartient à une seule opération à la fois. Une opération réussie rend sa session au pool. Une erreur ou annulation la ferme pour éviter de réutiliser des handles abandonnés. Les requêtes ne sont pas rejouées automatiquement, particulièrement les écritures dont l’issue peut être inconnue. Les acquisitions attendent au plus le timeout SFTP configuré.

Chaque lecture/écriture SFTP porte sur 32 Kio. Les downloads utilisent un canal borné à deux blocs et arrêtent la lecture lorsque le client se déconnecte. Les uploads consomment le flux HTTP sans accumuler le fichier ; le navigateur envoie des Blob de 4 Mio et l’API refuse plus de 8 Mio par fragment. La progression correspond aux octets acquittés et synchronisés par le serveur. Le débit affiché est un débit d’octets confirmés, pas une estimation de trafic réseau instantané.

Les listings sont bornés à 20 000 entrées, servis par pages de 200 (maximum 500). Il n’y a pas encore de cache ni d’index global ; le filtre de l’interface porte explicitement sur la page affichée. Cette borne protège la mémoire, mais le listing SFTP est relu à chaque requête. Les tests sur des dossiers très volumineux restent à faire.

## Cohérence

Un mutex global sérialise les mutations de namespace et les sauvegardes. Les fragments utilisent un verrou distinct par transfert et peuvent circuler en parallèle. L’éditeur compare un SHA-256 du contenu avant sauvegarde et immédiatement avant remplacement. Les autres opérations utilisent une version de métadonnées taille/mtime/mode : elle ne détecte pas toutes les modifications externes de même taille dans la même seconde.

Les fragments sont écrits dans `.glassftp/uploads/<UUID>`. Un `fsync` précède l’enregistrement de l’offset SQLite (`WAL`, `synchronous=FULL`). Après une interruption, l’offset journalisé fait autorité ; le prochain fragment réécrit la partie non confirmée et tronque sa queue éventuelle. La finalisation journalise `committing`, renomme sans écraser, puis enregistre `complete`. Un arrêt dans cette fenêtre conserve l’état incertain pour inspection ; aucune suppression automatique de ces fichiers n’est autorisée.

La corbeille journalise le chemin original avant le renommage SFTP. Une panne peut laisser une entrée `pending` : elle est conservée. SFTP et SQLite ne forment pas une transaction distribuée. La restauration et la suppression nécessitent parfois une réconciliation administrateur après une interruption entre ces deux systèmes.

SSE invalide les vues des autres navigateurs ; il ne constitue pas un verrou distribué. Un rafraîchissement à la reprise de focus et toutes les 60 secondes (hors onglet masqué/édition non sauvegardée) détecte les changements externes. Une seule instance de backend doit écrire dans cet espace.

## Décisions restantes

L’adresse Debian, le nom DNS, les sous-réseaux autorisés et le certificat interne relèvent de votre environnement et ne sont pas devinés. La qualification de production devra mesurer les limites de concurrence, les gros transferts et la charge réelle avant toute promesse de débit.
