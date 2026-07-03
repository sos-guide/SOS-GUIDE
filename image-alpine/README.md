# Image Alpine Linux — SOS-GUIDE (Raspberry Pi 4, diskless)

Image `.img` **bootable et reproductible** : Alpine Linux *diskless* (OS chargé en
RAM, carte SD préservée) embarquant l'application SOS-GUIDE (binaire Rust statique
aarch64-musl) et son portail web hors-ligne.

## Caractéristiques

- **OS en RAM** (Alpine diskless) : le système se charge entièrement en mémoire au
  boot. La carte SD n'est pas un point de corruption pour l'OS.
- **2 partitions** : `SOSBOOT` (FAT32, firmware Pi + kernel + apkovl, **ro**) ·
  `SOSDATA` (ext4, **montée ro**, repassée rw ponctuellement par `sos-commit-db`).
- **App embarquée via apkovl** (chargée en RAM au boot) : `sos-guide` (2,2 Mo,
  statique, zéro dépendance) + `sos-cli` + assets web (29 langues). Pas de `.apk`
  signé ni d'APKINDEX → 100 % hors-ligne, déterministe.
- **Service OpenRC** `sosguide` activé au boot, supervisé (`respawn`), bind `:80`.
- **Persistance v2 (modèle SOSDATA ro)** : la base Redb de **travail** vit en
  **tmpfs/RAM** (`/run/sosguide`) — zéro usure SD ; chaque écriture admin est
  **snapshotée** vers l'**instantané durable** sur SOSDATA
  (`/var/lib/sosguide/state/`) via `sos-commit-db` (remonte rw → copie atomique →
  re-verrouille ro). Au boot, l'instantané est **restauré** : config, identité
  Ed25519 et secrets retrouvés sans reconfiguration. SOSDATA reste ro le reste
  du temps. Validé bout-en-bout (l'identité survit à la perte du tmpfs).
- Sous-systèmes réseau (`SOS_NET_MODE`/`RADIO`/`GW`) **OFF** (matériel différé).

## Partitionnement (MBR)

| Part | Label | FS | Taille | Montage | Rôle |
|---|---|---|---|---|---|
| p1 | SOSBOOT | FAT32 | 512 Mo | `/media/mmcblk0p1` | firmware Pi4, kernel, modloop, cache apks, `*.apkovl.tar.gz` |
| p2 | SOSDATA | ext4 | 1 Go | `/var/lib/sosguide` (**ro**) | `state/` (instantané Redb durable), `tiles/` |

> **v2 (cette image) — modèle « SOSDATA ro » implémenté.** La persistance de
> l'app a été scindée par medium :
> - **État runtime** (sessions admin, inbox mesh) : déjà en **RAM** dans l'app.
> - **Base Redb de travail** : en **tmpfs** (`/run/sosguide`) — rw, rapide, zéro
>   usure SD ; perdue au reboot (rebâtie depuis l'instantané).
> - **Instantané durable** (config, identité Ed25519, mot de passe admin, alerte,
>   bulletins, registre de confiance) : `state/sos-guide.redb` sur **SOSDATA ro**,
>   écrit **uniquement** lors d'une action admin via `sos-commit-db` (fenêtre rw
>   en millisecondes), restauré au boot.
>
> Côté code : `sos-storage::Store::open_durable(working, target, commit_cmd)` +
> `persist_durable()` après chaque écriture ; rétrocompatible (sans
> `SOS_DB_DURABLE`, comportement historique du déploiement Debian). Variables :
> `SOS_DB` (travail/tmpfs), `SOS_DB_DURABLE` (instantané/SOSDATA),
> `SOS_COMMIT_CMD` (helper privilégié).
>
> - **Tuiles OSM** (gros fichiers binaires, pas un instantané) : restent sur
>   SOSDATA et sont écrites lors d'une **fenêtre rw** sérialisée. Le portail
>   encadre le téléchargement (`/api/admin/map`) et la purge (factory-reset) par
>   un garde RAII `RwWindow` : `sos-rw open` (remonte rw) → écriture → `sos-rw
>   close` (sync + remonte ro), **garanti au `Drop`** (y compris sur erreur).
>   Variable `SOS_RW_CMD` ; sérialisé par `tiles_lock`.
>
> **« ro 100 % » atteint** : en exploitation SOSDATA est montée **ro** en
> permanence ; les seules fenêtres rw sont les instantanés Redb (ms) et les
> écritures de tuiles (action admin rare).
>
> - **Garde-fou ro périodique (`crond`)** : `sos-ro-guard` s'exécute chaque
>   minute. S'il trouve SOSDATA montée rw **sans fenêtre légitime en cours**
>   (marqueur `/run/sos-rw.active` absent ou périmé, TTL 300 s), il la
>   **re-verrouille en ro**. Couvre le cas résiduel d'un `SIGKILL` du nœud
>   *pendant* une fenêtre. `sos-rw`/`sos-commit-db` posent le marqueur le temps de
>   leur fenêtre pour ne jamais être interrompus. Services `syslog` (journal) +
>   `crond` activés dans l'apkovl. Exposition rw d'une fenêtre orpheline bornée
>   à ≤ 60 s (cas sans marqueur) ou ≤ TTL (marqueur périmé).

## Construire l'image (reproductible)

Entrées épinglées : `pinned/alpine-rpi.version` + `pinned/alpine-rpi.sha256`.
Build dans un conteneur `alpine:3.21` (outils `mtools`/`e2fsprogs`/`sfdisk`),
sans toucher l'hôte ni nécessiter root.

```sh
# 1. binaires statiques aarch64-musl (sur l'hôte, rustup + cible musl)
cd ../sosguide
RUSTLLD=$(find ~/.rustup/toolchains -name rust-lld | head -1)
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER="$RUSTLLD" \
RUSTFLAGS="-C linker-flavor=ld.lld -C link-self-contained=yes" \
  cargo build --release --target aarch64-unknown-linux-musl -p sos-guide -p sos-cli

# 2. (ré)assembler overlay/ avec les binaires + web, puis :
cd ../image-alpine
docker run --rm -v "$PWD":/build -w /build alpine:3.21 sh assemble.sh
```

### Paquets WiFi hors-ligne (`boot-extra/aarch64/`)

`wpa_supplicant` + `iw` et leurs deps sont installés **au boot** depuis
`/media/mmcblk0p1/extra/*.apk` (cf. `sos-wifi`). Ils **doivent** être en
**aarch64** : un téléchargement depuis l'hôte x86_64 *sans* forcer l'architecture
produit des `.apk` x86_64 que le Pi rejette (`conflicts: musl … x86_64`) → WiFi
mort. Re-télécharger ainsi (et **ne pas** inclure `musl`, déjà dans la base) :

```sh
docker run --rm -v "$PWD/boot-extra/aarch64":/out alpine:3.21 sh -c '
  printf "%s\n" https://dl-cdn.alpinelinux.org/alpine/v3.21/main \
                https://dl-cdn.alpinelinux.org/alpine/v3.21/community > /etc/apk/repositories
  apk update --allow-untrusted >/dev/null 2>&1
  rm -f /out/*.apk
  apk fetch --arch aarch64 --allow-untrusted --no-cache -R -o /out \
    wpa_supplicant wpa_supplicant-openrc iw
  rm -f /out/musl-*.apk'
# vérifier : chaque .apk doit annoncer arch = aarch64 (ou noarch)
for f in boot-extra/aarch64/*.apk; do
  echo "$f -> $(tar -xzOf "$f" .PKGINFO | grep ^arch)"; done
```

Sortie : `out/sosguide-<ver>.img` + `.img.sha256`. Déterminisme appliqué :
`SOURCE_DATE_EPOCH`, tar `ustar --sort`, `gzip -n`, FAT `-N <serial figé>`, ext4
`-U <uuid figé> -E hash_seed=… lazy_*_init=0`, table MBR `label-id` figé.

## Mises à jour OTA de la flotte (« pull » signé)

Le binaire applicatif est mis à jour **sans reflasher**, par un modèle **pull
signé** adapté au diskless (OS en RAM). Le binaire d'**usine** reste cuit dans
l'apkovl (anti-bricage) ; un **slot OTA** sur la FAT le surcharge au boot s'il
est vérifié et plus récent.

**Chaîne (côté opérateur) :**

```sh
just release-keygen                 # une seule fois : release.key (privée, locale)
                                    # + overlay/etc/sosguide/release.pub (épinglée)
just publish-release 0.3.0          # build musl + signe → publish/{manifest.json,sos-guide.bin}
# servir publish/ sur un HTTP (ou hidden service Tor) joignable par l'Ethernet des bornes
```

Puis, sur chaque carte (partition **SOSBOOT**, éditable sans reflash), activer
dans `update.conf` :

```
ENABLED=1
URL=http://<ton-serveur>/sosguide          # sert manifest.json + sos-guide.bin
# CURL_OPTS=--socks5-hostname 127.0.0.1:9050   # si URL .onion (Tor)
```

**Côté borne (automatique) :** `crond` lance `sos-update` (~30 min) →
`sos-cli update` télécharge le manifeste + le binaire via `curl`, **vérifie
signature Ed25519 + empreinte SHA-256** (clé épinglée), **refuse tout
downgrade**, écrit le slot FAT (A/B) puis **reboote**. Au boot,
`sos-boot-select` réinstalle le slot s'il vérifie ; `sos-update-confirm` sonde
`/api/status` — **échec santé ⇒ rollback automatique** vers la version
précédente. Pour mettre à jour **toute la flotte** : publie un seul manifeste
signé, chaque borne le prend à son prochain check (aucune action par nœud).

Fichiers : `usr/local/sbin/{sos-update,sos-apply-update,sos-boot-select,sos-update-confirm}`,
clé/manifeste d'usine dans `etc/sosguide/`, conf sur la FAT `update.conf`.
**Tant que `release.pub`/`update.conf` ne sont pas fournis, l'OTA est inerte.**

## Flasher

```sh
xz -dk out/sosguide-<ver>.img.xz
sha256sum -c out/sosguide-<ver>.img.sha256
sudo dd if=out/sosguide-<ver>.img of=/dev/sdX bs=4M conv=fsync status=progress
```

Après flash : booter le Pi 4 → l'OS monte en RAM, OpenRC lance `sosguide` →
portail sur `http://<ip-du-pi>/`. La partition `SOSDATA` étant figée à 1 Go dans
l'image master, l'étendre à la carte si besoin :
`parted /dev/mmcblk0 resizepart 2 100% && resize2fs /dev/mmcblk0p2`.

## Limites connues (v2)

- **App en root** : `sos-commit-db` et `sos-rw` sont appelés directement (pas de
  `doas`), car l'app tourne en root (appliance mono-usage, bind `:80`). Pour un
  modèle non-root, préfixer `SOS_COMMIT_CMD`/`SOS_RW_CMD` par `doas` + règles
  `doas.d` ciblées.
- **Fenêtre rw + `SIGKILL`** : couvert par le **garde-fou ro périodique**
  (`sos-ro-guard` via `crond`, cf. encadré persistance). Exposition rw d'une
  fenêtre orpheline bornée à ≤ 60 s (sans marqueur) ou ≤ TTL 300 s (marqueur
  périmé), sans corruption (ext4 journalisé). Résiduel théorique : la minute
  entre deux passages du garde-fou.
- Pas d'auto-grow ni de `fsck` au boot (le journal ext4 couvre la coupure ;
  ajouter `e2fsprogs` au cache apks pour ces deux fonctions).
- Reproductibilité : flags appliqués ; une vérif bit-à-bit stricte demande de
  neutraliser les mtimes résiduels copiés par `mcopy` (mineur).
