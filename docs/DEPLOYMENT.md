# Installation sur Debian 13

Cette procédure est fournie pour une machine Debian dédiée ou un serveur dont l’administrateur adapte les comptes, les chemins, le DNS et les restrictions réseau. Les exemples ne sont pas appliqués automatiquement. La qualification réelle sur Debian reste obligatoire avant usage quotidien.

## 1. Préparer et compiler

Installer les mises à jour de sécurité puis les outils de compilation :

```sh
sudo apt update
sudo apt install build-essential pkg-config ca-certificates curl git openssh-server nginx sqlite3 nftables
```

Installer Rust stable ≥ 1.92 avec la méthode officielle de votre organisation. Compiler sur Debian pour l’architecture du serveur (le binaire macOS produit pendant le développement n’est pas déployable sur Linux). Prévoir environ 8 à 10 Go libres pour les dépendances et les builds.

```sh
rustup target add wasm32-unknown-unknown
cargo fetch --locked
cargo install wasm-bindgen-cli --version 0.2.130 --locked --root .tools
cargo test --locked
bash scripts/build-web.sh
cargo build --locked --release -p glassftp-server
```

La résolution est verrouillée dans Cargo.lock. Pour une compilation hors ligne, préparer auparavant le toolchain, les dépendances (`cargo vendor` si nécessaire) et wasm-bindgen. L’exécution de l’application ne nécessite aucun accès Internet.

## 2. Séparer les comptes et répertoires

```sh
sudo useradd --system --user-group --home-dir /var/lib/glassftp --shell /usr/sbin/nologin glassftp
sudo useradd --system --user-group --home-dir /shared --shell /usr/sbin/nologin glassftp-sftp
sudo passwd glassftp-sftp
sudo install -d -o root -g root -m 0755 /srv/glassftp
sudo install -d -o glassftp-sftp -g glassftp-sftp -m 0750 /srv/glassftp/shared
sudo install -d -o root -g root -m 0755 /opt/glassftp/bin /opt/glassftp/dist
sudo install -d -o glassftp -g glassftp -m 0700 /var/lib/glassftp
sudo install -d -o root -g glassftp -m 0750 /etc/glassftp
sudo install -o root -g root -m 0755 target/release/glassftp-server /opt/glassftp/bin/glassftp-server
sudo cp -r dist/. /opt/glassftp/dist/
sudo chown -R root:root /opt/glassftp
```

Choisir un mot de passe unique via l’invite `passwd`, jamais dans une commande passée en argument ou dans Git. Le compte HTTP `glassftp` ne doit pas appartenir au groupe `glassftp-sftp`. Tous les parents du chroot doivent appartenir à root sans écriture de groupe/autres. La racine SFTP applicative est `/shared`, distincte du chemin Debian `/srv/glassftp/shared`.

## 3. Configurer OpenSSH

Adapter puis installer `deploy/openssh/glassftp.conf` dans `/etc/ssh/sshd_config.d/70-glassftp.conf`. Vérifier que le service principal inclut bien ce répertoire. Le compte partagé est limité à internal-sftp, sans shell, forwarding ni connexion depuis le LAN. L’administration SSH existante reste séparée.

```sh
sudo install -d -m 0755 /etc/ssh/sshd_config.d
sudo install -o root -g root -m 0644 deploy/openssh/glassftp.conf /etc/ssh/sshd_config.d/70-glassftp.conf
sudo /usr/sbin/sshd -t
sudo /usr/sbin/sshd -T -C user=glassftp-sftp,host=localhost,addr=127.0.0.1 | grep -E '^(chrootdirectory|forcecommand) '
```

La configuration effective doit afficher ces deux valeurs (l’ordre peut varier) :

```text
chrootdirectory /srv/glassftp
forcecommand internal-sftp -d /shared -u 0027
```

Si elle affiche `none`, le fichier n’est pas pris en compte ou une autre règle correspondante prend priorité. Vérifier l’inclusion de `/etc/ssh/sshd_config.d/*.conf` dans la section globale de `/etc/ssh/sshd_config` et les autres règles `Match` avant de recharger. Avec ce chroot, `/shared` dans SFTP désigne `/srv/glassftp/shared` sur Debian. Un `ls /shared` exécuté au prompt Linux ne vérifie pas le répertoire partagé du chroot. Le compte à shell `nologin` doit utiliser `internal-sftp` comme prévu.

Après validation des deux valeurs et succès de `sshd -t` :

```sh
sudo systemctl reload ssh
sudo ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub -E sha256
```

Garder la session SSH d’administration ouverte et tester une deuxième session avant de se déconnecter. Copier l’empreinte obtenue **directement sur le serveur** dans `SFTP_HOST_KEY_SHA256`. Ne pas approuver aveuglément une clé obtenue par le réseau. Si plusieurs algorithmes de clé hôte sont proposés, vérifier que le client négocie bien la clé épinglée ; utiliser la clé Ed25519 de préférence. La rotation des clés exige une mise à jour explicite de la configuration et un redémarrage de GlassFTP.

Les répertoires `.glassftp/uploads` et `.glassftp/trash` sont créés via SFTP au premier démarrage avec mode 0700. Ils doivent rester sur le même filesystem que les fichiers partagés. Ne pas y placer de montage distinct. Les symlinks ne sont pas utilisables depuis l’application.

## 4. Configuration et secrets

Installer `.env.example` comme `/etc/glassftp/glassftp.env`, mode 0640, propriétaire root:glassftp. Remplacer le DNS, la racine, l’empreinte et les paramètres adaptés. Enregistrer le mot de passe via un éditeur administrateur dans `/etc/glassftp/sftp-password`, également root:glassftp et 0640. Le fichier doit contenir uniquement le mot de passe. Ne pas stocker une deuxième copie dans `.env` ou les logs.

```sh
sudo chown root:glassftp /etc/glassftp/glassftp.env /etc/glassftp/sftp-password
sudo chmod 0640 /etc/glassftp/glassftp.env /etc/glassftp/sftp-password
```

`APP_BASE_URL` doit correspondre exactement à l’origine utilisée par les navigateurs, par exemple `https://glassftp.lan`. Pas de sous-chemin. Le backend refuse une écoute non loopback et un nom d’hôte HTTP inattendu.

## 5. systemd

```sh
sudo install -m 0644 deploy/systemd/glassftp.service /etc/systemd/system/glassftp.service
sudo systemctl daemon-reload
sudo systemctl enable --now glassftp
sudo systemctl status glassftp
sudo journalctl -u glassftp -n 50 --no-pager
```

Le service n’a accès en écriture qu’à son état SQLite, et aucun accès direct à `/srv/glassftp`. `IPAddressAllow=localhost` convient au SFTP installé sur cette machine. Un autre hôte SFTP nécessiterait de modifier explicitement le durcissement et la politique réseau.

## 6. HTTPS et Nginx

Créer un enregistrement DNS interne `glassftp.lan` pointant sur l’IP LAN du serveur. Utiliser votre autorité de certification interne pour un certificat comportant ce nom dans le SAN. Installer la chaîne et la clé dans `/etc/glassftp/tls/` (clé lisible par root uniquement). Distribuer le certificat **de l’autorité** dans le magasin de confiance système de chaque poste ; vérifier les procédures propres à Firefox si celui-ci n’utilise pas le magasin système. Ne pas demander aux collègues de contourner une alerte de certificat.

Adapter `deploy/nginx/glassftp.conf`, en particulier le sous-réseau `192.168.10.0/24`, le hostname et les certificats. Installer le fichier dans `/etc/nginx/conf.d/`, puis :

```sh
sudo nginx -t
sudo systemctl reload nginx
```

Le buffering est désactivé dans les deux directions. La limite de 8 Mio concerne chaque fragment HTTP, et permet des fichiers de plusieurs Go. Les délais mesurent une période sans activité ; ils ne limitent pas la durée totale d’un transfert qui progresse. Le heartbeat SSE maintient les connexions ouvertes. Les assets sont servis localement ; cette version choisit `no-store` pour éviter les mélanges de versions JS/WASM. Un cache avec noms d’assets empreintés pourra être ajouté ultérieurement.

Nginx doit être l’unique entrée LAN. Ne pas ajouter de CORS générique. Adapter les serveurs par défaut pour rejeter les noms inconnus sans perturber les autres sites de la machine.

## 7. Pare-feu

`deploy/nftables.example.nft` est un **exemple à fusionner**, pas un script à exécuter tel quel. Il n’ouvre HTTPS qu’au LAN prévu, SSH à l’administration, conserve loopback et les connexions établies, et bloque le reste. Il ne contient pas `flush ruleset`. Prévoir les autres services, le port SSH réel et IPv6 ; aucune plage IPv6 LAN n’est autorisée par défaut.

Faire valider le fichier avec `nft -c -f fichier-adapte.nft`, garder une console de secours, prévoir un retour arrière temporisé selon vos procédures et vérifier l’accès SSH depuis une deuxième session avant de rendre les règles persistantes. Aucun changement de pare-feu n’a été effectué pendant ce développement.

## 8. Recette avant ouverture à l’équipe

- Vérifier `ss -ltnp` : GlassFTP uniquement sur 127.0.0.1:8080 ; aucun port de fixture.
- Tester HTTPS depuis un poste LAN autorisé et le refus depuis un réseau non autorisé.
- Vérifier le refus d’une mauvaise empreinte SSH, puis restaurer la configuration correcte.
- Exécuter les cas de `docs/STATUS.md` contre votre OpenSSH, dans une racine de recette jetable.
- Faire upload/download d’un fichier de plusieurs Go et comparer SHA-256 ; surveiller RSS, espace disque et débit réel.
- Lancer cinq sessions avec transferts concurrents, interrompre le réseau puis redémarrer le backend.
- Tester les conflits d’édition, la corbeille, la restauration et une sauvegarde complète.

La fixture automatisée valide le protocole, mais ne remplace pas cette recette Debian/Nginx/OpenSSH.

## Sauvegarder, restaurer et mettre à jour

La corbeille n’est pas une sauvegarde. Sauvegarder ensemble les fichiers SFTP (y compris `.glassftp/trash`), les métadonnées SQLite, la configuration et les secrets dans un stockage chiffré soumis à contrôle d’accès. Pour une sauvegarde cohérente simple, arrêter GlassFTP et suspendre les clients SFTP externes le temps d’un snapshot filesystem. Pour une sauvegarde SQLite à chaud, utiliser l’API backup ou `sqlite3 ... '.backup ...'`, jamais copier uniquement le `.sqlite` en ignorant son WAL.

Avant mise à jour, construire/tester dans un dossier séparé, sauvegarder l’état, arrêter le service, remplacer **ensemble** binaire et `dist`, puis redémarrer et vérifier `/health` via Nginx. Conserver la version précédente pour rollback. Le format SQLite V1 est une table de documents versionnée par le code ; aucune migration incompatible n’est fournie à ce stade.

Après une panne pendant `committing`, arrêter les écritures et comparer le journal SQLite, le temporaire UUID et la destination. Ne jamais supprimer une destination sur la seule base de sa taille. Si le renommage a eu lieu, conserver le fichier final et réconcilier le journal après vérification du contenu ; si le temporaire existe encore, conserver les deux jusqu’à résolution. Les orphelins issus de copie/édition interrompue peuvent nécessiter un nettoyage administrateur dans `.glassftp/uploads`. Ne pas supprimer automatiquement tout le répertoire.
