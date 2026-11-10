# Modèle de sécurité V1

## Frontière de confiance

Il n’y a pas d’identité individuelle. Toute personne pouvant atteindre Nginx depuis une adresse LAN autorisée peut utiliser les pouvoirs du compte SFTP partagé. On ne peut pas attribuer fiablement les modifications à un collègue. Les contrôles Host/Origin protègent le navigateur contre des pages tierces et le DNS rebinding ; ils ne bloquent pas un client HTTP malveillant déjà présent sur le LAN.

HTTPS, filtrage Nginx/nftables, backend loopback, compte de service sans privilèges et compte SFTP chrooté sont obligatoires. Les paramètres réseau réels doivent être validés par l’administrateur. Le backend ne fait pas confiance à X-Forwarded-For pour autoriser une requête.

## SSH et fichiers

L’empreinte SHA-256 est obligatoire et contrôlée avant envoi du mot de passe. Les certificats SSH sont refusés dans ce mode d’épinglage. Aucun secret n’est sérialisé dans les modèles publics ni journalisé par l’application. Les secrets en mémoire ne sont pas actuellement protégés par un conteneur zeroize spécifique ; éviter les core dumps et conserver les droits minimaux.

`VPath` valide les composants et la désérialisation. L’application vérifie chaque composant distant avec LSTAT et refuse les liens symboliques et les fichiers spéciaux. Les répertoires internes sont contrôlés à chaque acquisition de connexion. Les chemins doivent être UTF-8 ; les noms de fichiers non UTF-8 ne sont pas pris en charge.

SFTP v3 ne fournit pas de primitives équivalentes à `openat2(RESOLVE_BENEATH)` pour éliminer la course entre LSTAT et OPEN. Un chroot OpenSSH contenant uniquement l’espace dédié limite cette course au niveau OS. Le compte SFTP de service ne doit pas être confié directement aux collègues ; les administrateurs et autres programmes capables de modifier cet arbre sont considérés de confiance. Les hardlinks préexistants ne sont pas détectables comme des liens externes par ces attributs : ne pas en introduire vers des données sensibles. Une racine applicative à l’intérieur d’un chroot plus large ne protège pas seule contre un administrateur malveillant créant des liens pendant les vérifications.

## Intégrité et concurrence

Les créations et déplacements ne remplacent pas une destination existante. Le serveur cible doit respecter la sémantique SFTP v3 de RENAME sans écrasement. Les sauvegardes de texte utilisent l’extension POSIX rename ; pas de fallback vers une suppression suivie d’un renommage. `fsync` est requis. Cela protège le contenu du fichier, mais la durabilité du répertoire lors d’une coupure électrique reste celle du filesystem distant et du serveur SSH ; aucun `fsync` de répertoire portable n’est promis.

Une seule instance backend est autorisée. Le mutex protège les mutations venant de GlassFTP, pas les autres clients SFTP. SHA-256 protège les conflits d’édition déjà visibles lors des comparaisons. Il reste une fenêtre entre le dernier contrôle et le rename pour un écrivain externe. Les versions de métadonnées pour les autres actions sont plus faibles et peuvent manquer une modification de même taille/mtime. Pour la V1, les écritures régulières doivent passer par GlassFTP.

## Aperçus

Les types MIME sont actuellement déduits du nom, pas détectés par signature. L’aperçu inline est limité à une liste fermée de formats natifs (images raster, PDF, audio/vidéo), avec `nosniff` et CSP sandbox. Les SVG, HTML et JS sont téléchargés comme pièces jointes. Les textes sont UTF-8, sans octet nul, limités à 1 Mio et rendus avec des nœuds texte échappés. La coloration est lexicale, sans interpréteur ni injection HTML. Le Markdown est montré comme source.

L’éditeur et les téléchargements ne permettent ni exécution sur le serveur ni lancement arbitraire d’une URL vscode://. L’action VS Code explique le téléchargement et l’ouverture manuelle. Aucun helper ou extension n’est installé. Remote SSH nécessiterait un compte distinct autorisé au shell ; ce n’est pas compatible avec le compte de service `internal-sftp` tel que fourni.

## Limites de ressources

Concurrence des transferts, taille par fichier, corps HTTP, éditeur et listing sont bornés. Les exemples Nginx ajoutent des limites de requêtes et connexions. Prévoir un quota filesystem pour le compte SFTP : la limite par fichier n’empêche pas un utilisateur du LAN de remplir le stockage avec de nombreux fichiers.

Pas de purge automatique de la corbeille. Les uploads abandonnés expirent après sept jours sans fragment confirmé ; les finalisations incertaines sont conservées. Les journaux Nginx ne doivent pas enregistrer les query strings contenant des noms confidentiels. Les logs applicatifs enregistrent les chemins modifiés, sans contenu ni identifiants ; les traiter comme des données internes.

## Avant production

Exécuter un audit des dépendances Rust et du toolchain dans l’environnement de build, tester les configurations réelles, les mises à jour OpenSSH/Nginx et les cas réseau hostiles. Aucun audit externe ni mesure de charge multi-Go n’a été réalisé. Les tests locaux ne constituent pas une certification de sécurité.
