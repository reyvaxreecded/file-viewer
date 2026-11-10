# API v1

Les chemins sont virtuels (`/`, `/Documents/note.txt`), décodés une seule fois par l’extracteur HTTP. Aucun préfixe du filesystem Debian n’est exposé. Les composants `..`, `.`, vides, les caractères de contrôle, antislashs et noms commençant par `.glassftp` sont interdits. Les liens symboliques et fichiers spéciaux sont refusés.

Les écritures exigent `Origin` égal à `APP_BASE_URL` et `X-GlassFTP: 1`. Ce n’est pas une authentification ; les restrictions réseau sont obligatoires. Aucun CORS ouvert. Les corps JSON sont limités à 8 Mio et les textes éditables à 1 Mio.

| Méthode et chemin | Entrée | Résultat |
| --- | --- | --- |
| GET `/health` | — | Connexion SFTP vérifiée, `{status,storage}` |
| GET `/api/v1/files` | `path=/`, `offset=0`, `limit=200`, `q=` optionnels | `{path,entries,total,offset,limit}` ; filtre `q` dans ce dossier |
| GET `/api/v1/files/info` | `path` | `Entry` |
| POST `/api/v1/folders` | `{path}` | 201 |
| PATCH `/api/v1/files/rename` | `{source,destination,version}` | 204, destination absente obligatoire |
| POST `/api/v1/files/move` | Même corps | 204 |
| POST `/api/v1/files/copy` | Même corps ; fichier ordinaire seulement | 201 |
| POST `/api/v1/files/trash` | `{path,version}` | `TrashItem` |
| GET `/api/v1/trash` | — | Liste des métadonnées de corbeille |
| POST `/api/v1/trash/{uuid}/restore` | `{destination}` | 204, aucun écrasement |
| DELETE `/api/v1/trash/{uuid}` | — | 204, suppression définitive récursive bornée |
| GET `/api/v1/text` | `path` | `{path,content,version}` ; version SHA-256 |
| PUT `/api/v1/text` | `{path,content,version}` | Même structure après sauvegarde |
| GET/HEAD `/api/v1/download` | `path`, `inline=false` | Flux, 200/206/416, `Accept-Ranges: bytes` |
| GET `/api/v1/events` | — | SSE `change`, JSON `{path}` ; heartbeat |
| POST `/api/v1/uploads` | `{path,size}` | 201, transfert réservé |
| GET `/api/v1/uploads` | — | Transferts non finalisés partagés |
| GET `/api/v1/uploads/{uuid}` | — | État persistant |
| PATCH `/api/v1/uploads/{uuid}` | Corps binaire, `Upload-Offset`, `Content-Length` | Offset confirmé après `fsync` |
| POST `/api/v1/uploads/{uuid}/complete` | `{}` | État `complete`, après renommage |
| DELETE `/api/v1/uploads/{uuid}` | — | Annulation et nettoyage ; 204 |

`Entry` contient `path`, `name`, `kind` (`file`/`directory`), `size`, `modified` (secondes Unix), `mime` (déduit de l’extension), `version`. Les types sont déclarés dans `crates/shared`.

## Uploads

1. Réserver un chemin absent avec sa taille totale (maximum configurable, 100 Gio par défaut).
2. Envoyer des fragments contigus de 1 à 8 Mio. Un offset différent de l’offset confirmé produit 409. Après une réponse perdue, relire l’état avant de réessayer.
3. Ne pas envoyer deux fragments simultanément pour un même UUID. Plusieurs UUID sont indépendants.
4. Finaliser uniquement lorsque `offset == size`. Un fichier vide passe directement à la finalisation.
5. Annuler par DELETE pour libérer la réservation. Un transfert `committing` ne peut pas être annulé automatiquement.

L’interface reprend les transferts en conservant le même objet File dans l’onglet. Après fermeture du navigateur, le protocole permet la reprise par un client API fournissant exactement le même fichier ; l’interface indique explicitement cette limite. Elle ne suppose pas qu’un fichier resélectionné du même nom est identique.

Conflit de nom : conserver les deux (nouveau nom), ignorer, annuler. Le remplacement explicite pendant l’import n’est pas encore proposé. L’éditeur utilise un remplacement atomique OpenSSH après vérification d’empreinte.

Rétention : les transferts actifs sans fragment confirmé depuis sept jours sont annulés par une tâche exécutée toutes les quinze minutes. Les journaux `complete` sont retirés après un jour. Les états `committing` sont conservés. La corbeille n’est jamais purgée automatiquement.

## Erreurs et téléchargement

Les erreurs applicatives ont la forme `{code,message}` : 400 entrée invalide, 403 chemin/origine interdit, 404 absent, 409 conflit, 413 limite, 422 opération non supportée, 429 concurrence saturée, 503 SFTP indisponible. Les rejets d’extracteur Axum et les erreurs Nginx peuvent encore être du texte ; le frontend dispose d’un fallback HTTP.

Une seule plage HTTP est prise en charge, y compris `bytes=N-` et `bytes=-N`. Les plages multiples ou invalides reçoivent 416. L’ETag de métadonnées est faible ; une requête `If-Range` retourne la représentation complète. Un fichier modifié en place par un client SFTP externe pendant un download n’offre pas de snapshot cohérent.

Les fichiers HTML, JS, SVG et autres contenus actifs sont des pièces jointes, jamais du HTML inséré dans l’interface. Les aperçus autorisés disposent d’une CSP sandbox. Le texte est rendu comme texte échappé. Aucun rendu Markdown HTML ni transcodage multimédia.
