# État vérifiable — 10 novembre 2026

## Livraison actuelle

Première tranche verticale fonctionnelle étendue aux opérations courantes. Le projet n’est **pas encore déclaré prêt pour la production**, ni conforme à la totalité du cahier des charges.

| Phase | Réalisé | Vérification / reste |
| --- | --- | --- |
| Architecture | Workspace, contrats typés, pool SFTP asynchrone, journal SQLite, modèle de sécurité | Builds Rust/macOS et WebAssembly réussis ; choix et risques documentés |
| Fondation | Axum, configuration stricte, clé SSH épinglée, connexion par mot de passe, listing et navigation Leptos | Tests réels HTTP → Rust → SSH/SFTP, clés et identifiants erronés inclus |
| Interface | Dark/light, sidebar réductible, breadcrumb, grille/liste, détails, sélection multiple, menu contextuel, skeletons, erreurs et notifications | Vérification navigateur de la navigation, liste, sélection multiple, aperçu, édition, import et renommage |
| Fichiers | Upload par fragments, download streaming/Range, dossiers, rename/move, copie de fichiers, dépôt natif et interne, import de dossiers | 28 contrôles d’intégration ; import par sélecteur réellement vérifié dans le navigateur. Les dépôts OS et de dossiers nécessitent encore une matrice Windows/Linux/macOS |
| Fonctions avancées | Corbeille persistante, restauration, purge, favoris et récents locaux, texte/code, médias natifs, raccourcis, transferts | Éditeur sauvegardé depuis le navigateur ; coloration lexicale simple. Pas encore d’index global ni ZIP |
| Collaboration/sécurité | Mutex mutations, verrous de fragments, SHA-256 éditeur, SSE, Host/Origin, CSP, sandbox des contenus, bornes de ressources | Course de renommage, conflit d’édition, permissions, traversal, symlink et contenu actif testés. Limites face aux écritures SFTP externes documentées |
| Production | Exemples Nginx/systemd/OpenSSH/nftables, installation et sauvegardes | Transfert de 2 Gio mesuré localement ; pas de déploiement Debian effectué ni de qualification OpenSSH/Nginx |

## Tests exécutés

`cargo test --locked` : quatre tests Rust passent (validation de chemins, sérialisation et plages HTTP). `cargo check` du backend et les compilations release du backend et du WebAssembly passent. Le backend a été compilé pour macOS arm64. `node --check` valide les deux modules JavaScript.

`tests/integration.py --large` : **28 contrôles passent**, avec un fichier de **2 Gio**, **trois uploads simultanés**, et un scénario de **cinq clients simultanés** (deux uploads, deux downloads et un listing). Ils couvrent : refus du mauvais mot de passe et de la mauvaise clé hôte ; health SFTP ; masquage des internes ; traversal ; symlinks ; permissions ; absent ; Host/Origin ; création et collision ; offset invalide ; partiel invisible ; fragment interrompu ; arrêt avec SSE ouvert ; redémarrage/reprise ; finalisation ; comparaison des octets ; Range valide et invalide ; édition et conflit SHA-256 ; renommage concurrent ; copie ; corbeille/restauration et collision ; purge ; parallélisme ; fichier vide ; pièce jointe HTML sandboxée ; download interrompu ; gros transfert et mémoire.

Mesure du 10 novembre 2026 sur macOS arm64, backend en profil debug, SSH/SFTP Paramiko sur loopback : **2 147 483 648 octets** uploadés puis téléchargés, SHA-256 identique, en **24,85 s** au total. RSS du backend avant l’essai : **20 791 296 octets** ; maximum échantillonné : **21 446 656 octets (20,5 Mio)**. L’échantillonnage a lieu après chaque fragment upload de 4 Mio et chaque segment download de 64 Mio ; il ne capture pas nécessairement un pic très bref. Cette mesure concerne uniquement le processus Rust, pas le navigateur ou la fixture. Ce n’est pas une promesse de débit ou de consommation sur Debian/Nginx.

Le rapport de chaque exécution est écrit dans `test-results/integration.json` (non commité). Le serveur de test est Paramiko, avec extensions fsync et POSIX rename ; il n’est pas OpenSSH. Les traces « Connection reset by peer » de la fixture peuvent apparaître lors des tests de rejet et d’annulation : l’application ferme volontairement les sessions en erreur.

Le passage étendu a révélé qu’une connexion SSE persistante pouvait retenir indéfiniment l’arrêt HTTP. Les flux SSE se ferment maintenant sur SIGINT/SIGTERM ; les requêtes actives disposent de 75 secondes pour terminer. Un test ouvre explicitement SSE et vérifie l’arrêt propre avant de contrôler la reprise de l’upload après redémarrage.

La vérification navigateur a trouvé puis fait corriger deux expressions d’attribut Leptos contenant `>=`, interprétées initialement comme du texte. Le build corrigé a été rechargé et vérifié. L’import de `browser-note.txt` a atteint « Terminé » à 103/103 octets, puis le fichier a été renommé depuis le menu contextuel. Une modification de `exemple.rs` a été sauvegardée et sa nouvelle taille relue dans la liste. Aucun message d’erreur JavaScript n’a été observé pendant ces parcours.

Le déplacement par le menu Couper puis le bouton Coller a été validé entre deux dossiers. L’actualisation conserve désormais la sélection existante, vérifiée dans le navigateur. Affichage contrôlé à 1366 × 768 et 1920 × 1080 ; à 2560 × 1440, aucun débordement horizontal du document. Les thèmes clair et sombre ont été observés. Les raccourcis presse-papiers n’ont pas pu être entièrement vérifiés avec l’outil navigateur, qui intercepte ces combinaisons ; les actions de menu équivalentes fonctionnent. [Capture de la recette](preview.jpg).

## Limites fonctionnelles explicites

- Le filtre porte sur la page affichée. Pas de recherche globale indexée, initialisation d’index ni synchronisation d’index.
- Les listes sont bornées à 20 000 entrées côté serveur et paginées ; pas de virtualisation ni benchmark de dossier à cette limite.
- La copie récursive et le ZIP groupé ne sont pas implémentés. Le téléchargement se fait fichier par fichier.
- Import en conflit : conserver les deux, ignorer ou annuler ; pas de remplacement explicite à l’import. Les restaurations en collision sont refusées par défaut ; l’API permet un autre chemin.
- Reprise dans le même onglet avec l’objet File d’origine ; après fermeture, les offsets persistent mais la reprise graphique par resélection n’est pas encore implémentée.
- Renommage dans un dialogue, pas inline. Le drag & drop est implémenté mais sa qualification par navigateur/OS reste à faire.
- Prévisualisation des images raster, PDF et médias natifs ; pas de miniatures générées, de transcodage ou de SVG rendu. Pas de zoom dédié ; texte UTF-8 limité à 1 Mio, Markdown affiché en source.
- Le MIME est déduit de l’extension. Le highlighter est lexical, pas un moteur complet par langage. Pas encore de bouton copier le texte.
- VS Code propose des instructions de téléchargement/ouverture manuelle ; aucun protocole vscode://, helper, extension ou Remote SSH n’est configuré.
- Les favoris/récents peuvent devenir obsolètes après déplacement ou suppression. Ils restent locaux et peuvent être retirés manuellement.
- La disponibilité SFTP n’est pas un verrou distribué. Les mutations externes de même taille/mtime et la fenêtre finale de comparaison restent des limites connues.
- Les finalisations `committing` et certaines interruptions de corbeille nécessitent une réconciliation conservatrice ; elles ne sont pas auto-déclarées réussies.
- Nettoyage des uploads journalisés après expiration ; des temporaires orphelins de copie/édition interrompue peuvent nécessiter un nettoyage administrateur.

## Prochaine tranche recommandée

1. Recette OpenSSH/Debian 13 sous Nginx : flux multi-Go, mémoire RSS, cinq clients, arrêts réseau/électriques simulés et règles LAN.
2. Renforcer le journal de finalisation et la récupération de corbeille ; ajouter révisions applicatives de namespace et tests de crash injectés à chaque transition.
3. Reprise graphique après resélection avec vérification d’identité du fichier, remplacement d’import explicitement confirmé et alternative de restauration en collision.
4. Index de recherche avec invalidation et synchronisation externe, copies récursives, ZIP streaming, miniatures et améliorations d’édition/accessibilité.

Les mesures locales ci-dessus ne valent que pour le scénario exécuté. Les tests actuellement passés justifient une recette technique dans un environnement isolé, pas une mise en production sans validation supplémentaire.
