# /etc/sosguide — racine de confiance des mises à jour OTA

Deux fichiers (épinglés dans l'image, donc en RAM au runtime) gouvernent la
mise à jour OTA « pull » signée. **Tant qu'ils sont absents, l'OTA est inerte**
(la borne démarre toujours sur le binaire d'usine cuit dans l'apkovl).

| Fichier | Rôle | Produit par |
|---|---|---|
| `release.pub` | Clé **publique** Ed25519 de publication (PEM). Vérifie la signature de tout binaire OTA. **La clé privée ne quitte jamais ton poste.** | `just release-keygen` |
| `manifest.json` | Manifeste **d'usine** : version + empreinte du binaire livré dans cette image. Sert l'**anti-downgrade** (un OTA doit être ≥ cette version). | `just publish-release` (sign-update sur le binaire d'usine) |

Voir aussi, côté carte SD (partition SOSBOOT, éditable sans reflash) :
`update.conf` — `ENABLED` / `URL` / `CURL_OPTS`.

## Chaîne de confiance
1. Tu génères une paire de clés une fois (`release-keygen`) : `release.pub` va
   dans l'image, `release.key` (privée) reste chez toi.
2. À chaque version : `just publish-release` signe le binaire → `manifest.json`
   + `sos-guide.bin`, que tu sers sur ton serveur (HTTP/Tor).
3. Chaque borne (`sos-cli update`, via cron) télécharge, **vérifie signature +
   empreinte**, refuse tout downgrade, installe dans le slot FAT (A/B) et
   reboote. Échec de santé ⇒ rollback automatique.

`sos-cli` (le vérificateur/updateur) doit être présent dans l'image
(`/usr/local/bin/sos-cli`) — voir le `README.md` de `image-alpine/`.
