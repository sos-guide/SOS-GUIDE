# Changelog

Toutes les modifications notables de SOS-GUIDE sont consignées ici.

Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/),
versionnage [SemVer](https://semver.org/lang/fr/).

## [Non publié]

### Corrigé
- **Panique au démarrage (portail injoignable) — route axum 0.8 corrigée.** La
  route `/api/admin/groups/:id` utilisait la syntaxe de capture axum 0.7 (`:id`),
  rejetée par axum 0.8 : `Router::route` **paniquait à la construction**, tuant le
  nœud au boot (AP + DHCP OK mais rien sur le port 80). La compilation et les 147
  tests passaient car **aucun test ne construisait le routeur complet**. Corrigé en
  `{id}` + **test de non-régression** `router_builds_without_panicking` (construit
  le routeur complet). Confirmé sur Raspberry Pi réel (portail servi sur Wi-Fi ouvert).
- **Image Alpine : point de montage `/var/lib/sosguide` créé** dans l'overlay
  (`.keep`) — le `fstab` y monte SOSDATA (ro) mais le dossier n'existait pas, cassant
  la persistance. Ajout d'un **diagnostic de boot différé** (`sos-netmode` écrit
  `SOSGUIDE_DIAG.txt` sur la FAT : état du service, port, montage, log) pour
  diagnostiquer une borne sans écran ni SSH.
- **`image-alpine/assemble.sh` : synchro automatique du portail web** depuis
  `sosguide/web/` (source de vérité **unique**) vers l'overlay au moment du build,
  `data/config.json` (graine d'image) **préservée**. Corrige une dérive silencieuse :
  l'overlay était un snapshot figé — le correctif « clé WiFi » et d'autres évolutions
  (ex. groupes de ping) n'atteignaient pas l'image produit. L'overlay committé a été
  resynchronisé.

### Retiré
- **Machinerie « clé WiFi / WPA » entièrement retirée** (cohérence avec la
  décision 2026-06-28 « AP toujours ouvert »). L'assistant `/install` distribuait
  encore un `wifiPassword` + un QR `T:WPA` tout en affichant « WiFi ouvert » —
  contradiction supprimée. Détail : `sos-security::generate_wifi_key`/`WIFI_KEY_LEN`,
  l'enum `sos-network::plan::Security` (+ branche WPA de `hostapd_conf`), le champ
  `NetworkConfig.wifi_key`, la génération/rotation de `wifiPassword` du portail,
  et l'affichage de la clé dans `install.html`/`admin.html`. Le QR encode désormais
  le **réseau ouvert** (`WIFI:S:SOS-GUIDE;T:nopass;;`). Purge défensive de
  `wifiPassword` **conservée** (config héritée v2.5). Chaînes i18n `install`
  (`done_net`/`done_note`) et `admin` (`net_desc`/`net_wifi`/`net_hint`)
  **corrigées dans les 29 langues** (« réseau ouvert / aucun mot de passe ») ;
  4 clés i18n devenues orphelines (`net_wifikey`, `regenwifi_chk`,
  `js_sec_confirmwifi`, `js_sec_newwifi`) supprimées de toutes les langues.
- **Outillage d'image Raspberry Pi OS Lite supprimé** (`sosguide/image/` :
  `sosguide-harden.sh`, `sosguide-readonly.sh` overlayroot, `README.md`) —
  **superSédé par l'image produit Alpine diskless** (`image-alpine/`). Recettes
  `just image-harden`/`image-readonly` retirées ; ROADMAP Phase 6 (rootfs RO +
  `.img` reproductible) désormais **assurée par Alpine**. Le socle de dev Debian
  (`just install`/systemd) est conservé.
- **Site vitrine `webpage/` sorti du dépôt produit** (déplacé vers
  `sosguide.fr/webpage/`, ~87 Mo de PHP/PDF/vidéo) : le dépôt produit reste du
  code Rust + image, sans gros binaires marketing (ignoré via `.gitignore`).
- **`audit-control/settings.local.json`** : résidu obsolète purgé (typo
  `sosguie.fr`, chemins `sg-box`/`sg-claude`, commandes de setup ponctuelles).

### Ajouté
- **`sos-pay` — relais « Bitcoin tx over LoRa » (mode urgence), cœur gaté.**
  Nouveau crate isolé, **désactivé par défaut** (`SOS_PAY_MODE=off`), sans impact
  sur le portail vital. La borne est un **transporteur** de transactions signées,
  **jamais un portefeuille** (ni clé ni fonds) : un client signe sa tx, la borne la
  valide (format + plafond de taille `MAX_TX_BYTES`), la met en **file** bornée
  anti-doublon (`queue`), la **fragmente** en trames LoRa JSON compactes ~120 o +
  réassemblage (`frame`), sous une **politique alertes-first** rate-limitée
  (`policy` : jamais d'émission tant qu'une alerte occupe le canal), puis un
  **nœud-sortie** connecté la **diffuse** via `curl` → API publique (`broadcast`,
  mempool.space par défaut). Pur + testé (17 tests, dont le pipeline bout-en-bout) ;
  **live/matériel LoRa différé** (comme `sos-radio`). Décision produit dans `ASK.md`
  (Monero abandonné, Bitcoin conservé).
- **`sos-pay` câblé dans l'app + endpoints portail `/api/pay`.** L'app construit le
  relais selon `SOS_PAY_MODE` (`off` par défaut ⇒ **désactivé**, aucun impact vital ;
  `SOS_PAY_BROADCAST_API` optionnel). Deux routes : **`POST /api/pay`** (dépose une tx
  signée hex → validation + mise en file ; `202` en file, `200` doublon, `422` invalide,
  `503` si désactivé ou file pleine ; garde CSRF) et **`GET /api/pay`** (état public :
  `enabled` + `queued`/`pending` + liste `{id,size,status}`, **sans exposer les tx
  brutes**). Vérifié en natif (off = désactivé ; simulate = dépôt/doublon/invalide/file).
- **`sos-pay` ↔ `sos-radio` : maillage LoRa du paiement (créneau 3).** Le canal LoRa
  transporte désormais, **en best-effort et alertes-first**, des fragments de tx en
  plus des alertes. Émission : `submit_pay` fragmente la tx acceptée et pousse les
  fragments vers la radio (`NodeState.pay_tx`) ; statut → `relayed`. Réception :
  l'orchestrateur radio, en **sélection biaisée** (réception + alertes **avant**
  paiement), route une trame non-alerte vers un `Reassembler` et, à transaction
  complète, la met en file dans le relais partagé (`Relay::accept_raw`). App : relais
  **partagé** portail↔radio + canal de fragments. Vérifié en natif (POST → 3 fragments
  `sent=3` → statut `relayed`) + tests unitaires (réassemblage entrant, émission
  sortante). Workspace : 164 tests, clippy `-D warnings` propre.
  _Reste : **pilote LoRa `live`** (SX1276/Meshtastic) + **diffusion `live`** (`curl`
  côté nœud-sortie) — nécessitent le **matériel** pour être écrits et testés._
- **Mode point d'accès opérationnel dans l'image Alpine (AP ouvert + uplink
  Ethernet optionnel).** La borne diffuse désormais **son propre WiFi ouvert**
  (`SOS-GUIDE`) avec **portail captif** : un init `sos-netmode` monte l'AP via
  **hostapd** (radio, réseau ouvert, `ap_isolate`) + **dnsmasq** (DHCP +
  capture DNS totale `address=/#/10.0.0.1`), l'appli servant le portail (mode
  `off`). **`eth0` = uplink optionnel** : si un câble est présent → DHCP + mDNS
  **`sosguide.local`** (avahi, sans D-Bus) ; sinon **autonomie totale** (AP seul).
  **Aucun surf Internet** : `ip_forward=0` + `FORWARD DROP`, `eth0` jamais routé
  vers les clients. Conf éditable sur la carte (`ap.conf` : SSID/COUNTRY/CHANNEL,
  défaut `SOS-GUIDE`/FR/6). Paquets AP en aarch64 hors-ligne (`boot-extra`), le
  mode client de test (`sos-wifi`/`wifi.conf`) est **retiré**. Image
  `out/sosguide-3.21.7.img` reconstruite (+ `.img.xz` + checksums), **vérifiée**
  (FAT/apkovl/paquets) et **purgée de toute trace du réseau personnel du dev**.
  _Reste : test sur Pi réel (boot + diffusion AP + portail) — le `.img` ne peut
  être prouvé « opérationnel » que sur matériel._
- **Mises à jour OTA de la flotte (« pull » signé, image Alpine diskless).** Une
  borne se met à jour **sans reflasher** : `crond` lance `sos-update` (~30 min) →
  `sos-cli update` (nouvelle sous-commande) télécharge un **manifeste signé +
  binaire** via `curl` à l'URL configurée (Ethernet/Tor), **vérifie signature
  Ed25519 + empreinte SHA-256** (clé de publication épinglée), **refuse tout
  downgrade**, écrit un **slot A/B** sur la FAT puis **reboote**. Au démarrage,
  `sos-boot-select` installe le slot s'il vérifie et est ≥ usine (le binaire
  d'**usine** reste dans l'apkovl = anti-bricage) ; `sos-update-confirm` sonde
  `/api/status` et **rollback automatique** en cas d'échec santé. Pour MAJ
  **toute la flotte** : publier un seul manifeste signé, chaque borne le prend à
  son prochain check. Désactivé par défaut (`update.conf` `ENABLED=0`, inerte
  tant que clé + URL absentes). Outillage opérateur : `just release-keygen`,
  `just publish-release <version>`. Fichiers : `crates/cli` (`update`),
  `image-alpine/overlay/usr/local/sbin/sos-{update,apply-update,boot-select,update-confirm}`,
  `etc/sosguide/`, `boot/update.conf`, crontab, `init.d/sosguide` (start_pre/post).

### Corrigé
- **Associations & numéros d'aide : suivent le PAYS, plus la langue.** Les
  associations affichées sur le portail provenaient des fichiers de langue
  (`fr.json`) — donc **toujours françaises**, quel que soit le pays de
  déploiement. Elles sont désormais **configurables** : un **éditeur (nom +
  numéro, 1 à N lignes)** est ajouté à **`/install`** (nouvelle étape
  « Associations ») **et à `/admin`** (section dédiée avec sa propre
  sauvegarde). La saisie est persistée dans la **configuration du nœud**
  (`config.associations`, non secrète → présente dans la projection publique) ;
  le portail affiche ces associations **à la place** de la liste de la langue.
  **Repli** sur la liste de la langue **uniquement si rien n'est configuré**
  (rétro-compatibilité : un nœud FR sans saisie garde ses numéros nationaux).
  _Aucun changement Rust : la config est un JSON libre, l'install la persiste,
  `/api/admin/config` la fusionne, la projection publique conserve les clés non
  secrètes — `associations` transite de bout en bout sans nouveau code serveur._

### Modifié
- **Cohérence visuelle entre les pages (accueil ⇄ installation).** (1) La
  **barre de menu** de `/install` est désormais **strictement identique** à
  l'accueil (logo ⛑️ SOS-GUIDE, hamburger + tiroir, `A−/A+`, 🌐 langue, 🌙
  thème) — les styles de cette barre (et tiroir + modale de langue) sont
  **extraits dans `lib/sos-theme.css`** comme source unique partagée par toutes
  les pages. (2) `/install` débute par une **carte de présentation du projet**
  (1ʳᵉ étape « Le projet » : ce qu'est SOS-GUIDE, fonctionne sans Internet, 29
  langues, aucune donnée personnelle ; clés i18n `intro_*` avec repli FR).
  (3) Le **Morse** n'est plus une section figée de l'accueil mais un **outil à
  part entière** (carte carrée dans la grille d'outils → modale, comme les
  autres outils) ; ajouté pour **toutes les langues** (repli FR, libellés
  `tools.morse` dans `fr.json`).
- **Affichage du portail (accueil + installation).** (1) Le contrôle de **taille
  du texte** `A−/100%/A+` (+ bascule de thème) est placé **en haut** de la page
  d'installation, comme l'accueil. (2) La **carte OSM** est désormais
  **masquée sur l'accueil** et rendue **facultative à l'installation** (case à
  cocher « Afficher une carte du lieu » ; `mapScope=none` si décochée, sans
  téléchargement de tuiles). (3) La **barre du bas** a une **hauteur fixe** (ne
  s'agrandit plus au défilement) et ses onglets suivent l'**ordre de la page**
  (Outils · Urgences · Guide · Aides ; onglet « Carte » retiré). (4) Les **outils**
  sont des **cartes carrées** (icône + titre centrés). (5) Les sections
  **Urgences médicales**, **Situations collectives** et **Associations & numéros
  d'aide** passent de grilles de cartes à des **tableaux** cliquables (ligne →
  fiche détaillée ; numéro composable en `tel:` pour les associations).
  _Le libellé du toggle carte et les onglets du bas restent en français (à
  internationaliser ultérieurement)._

### Corrigé
- **Install impossible sur l'image Alpine (« administration non configurée ») —
  `flock -w` incompatible busybox.** Le helper de commit durable `sos-commit-db`
  utilisait `flock -w 5 9` (attente bornée) ; or le `flock` de **busybox** (base
  Alpine) **ne connaît pas `-w`** → le helper sortait en erreur **à chaque**
  écriture. Conséquence en chaîne : `Store::persist_durable` échouait, donc
  `save_config`/`save_admin_password` renvoyaient `Err` → le wizard `/install`
  échouait silencieusement et le mot de passe admin n'était jamais persisté →
  `/admin` répondait « administration non configurée ». Correctif : attente
  bornée (~5 s) émulée avec `flock -n` dans une boucle (portable busybox).
  **Vérifié** en conteneur Alpine (busybox `ash`) : commit OK lock libre,
  marqueur retiré au `trap`, et attente correcte sous contention. _Seul
  `sos-commit-db` utilisait `flock` (`sos-rw`/`sos-ro-guard` indemnes)._

### Ajouté
- **WiFi client configurable par fichier (image Alpine) — `SOSBOOT/wifi.conf`.**
  Le SSID/mot de passe/pays du mode client ne sont plus figés dans l'apkovl : le
  service `sos-wifi` lit `/media/mmcblk0p1/wifi.conf` (racine de la partition FAT
  `SOSBOOT`, **éditable depuis n'importe quel PC** sans recompiler) et génère la
  conf `wpa_supplicant` à chaud. Format `CLE=valeur` (`SSID`/`PSK`/`COUNTRY`),
  tolère les fins de ligne Windows (CR), SSID à espaces, réseau ouvert (`PSK`
  vide → `key_mgmt=NONE`). `SSID` vide ⇒ mode client simplement ignoré (pas
  d'erreur). L'ancien `/etc/wpa_supplicant/wpa_supplicant.conf` figé est retiré.
  `assemble.sh` dépose un `wifi.conf` documenté par défaut à la racine FAT.
  _Mode client = confort de TEST/bring-up ; en exploitation finale la borne sera
  en **point d'accès autonome** (aucun réseau à rejoindre), Ethernet/Tor/LoRa
  conservant leur fonction propre._

### Corrigé
- **WiFi inopérant au boot (image Alpine) — paquets de mauvaise architecture.**
  Les `.apk` embarqués dans `image-alpine/boot-extra/aarch64/` étaient en réalité
  des paquets **x86_64** (téléchargés sur l'hôte Manjaro sans forcer l'arch). Au
  boot du Pi (aarch64), `apk add --no-network` les rejetait tous
  (`conflicts: musl … so:libc.musl-x86_64.so.1`), donc `wpa_supplicant`/`iw`
  n'étaient jamais installés → pas d'association → `wlan0` `NO-CARRIER` → aucun
  bail DHCP (« pas d'IP obtenue », cf. `DIAG.txt`). Le driver `brcmfmac` + le
  firmware BCM4345/6 chargeaient pourtant correctement : la rupture était **uniquement**
  l'architecture des paquets. Correctif : re-téléchargement de la clôture en
  **aarch64** (`apk fetch --arch aarch64 -R wpa_supplicant wpa_supplicant-openrc iw`)
  et **retrait de `musl`** (déjà fourni par la base diskless, même version `r11` —
  source du conflit `world[]`). Image `out/sosguide-3.21.7.img` reconstruite,
  `.img.xz` + checksums régénérés. **Confirmé sur Pi 4 réel (2026-06-27)** :
  WiFi associé, portail joignable sur `http://192.168.1.133/`.

### Ajouté
- **Garde-fou ro périodique (`crond`) — image Alpine.** `sos-ro-guard` (cron
  chaque minute) re-verrouille SOSDATA en `ro` si trouvée `rw` **sans fenêtre
  d'écriture légitime en cours** (marqueur `/run/sos-rw.active` absent ou périmé,
  TTL 300 s) : couvre le cas résiduel d'un `SIGKILL` du nœud pendant un instantané
  Redb ou une écriture de tuiles. `sos-rw` et `sos-commit-db` posent/retirent le
  marqueur le temps de leur fenêtre (jamais interrompus). Services `syslog`
  (provide logger) + `crond` activés dans l'apkovl, crontab `/etc/crontabs/root`.
  Exposition rw d'une fenêtre orpheline bornée à ≤ 60 s (sans marqueur) ou ≤ TTL.
  **Page d'audit améliorée** (`audit-control/audit-gen.py`) : nouvelle vignette
  « Durcissement appliance — SOSDATA en lecture seule » (badges ✓/⚠, sceau
  `🔒 SOSDATA ro · n/n`) lue d'une section dédiée de `ROADMAP.md` et exclue du
  décompte d'étapes ; libellés nettoyés du markdown inline.
- **Fenêtre d'écriture rw pour les tuiles (`sos-portal`) — « SOSDATA ro 100 % ».**
  Garde RAII `RwWindow` encadrant les écritures de fichiers sur SOSDATA (montée
  ro) : `<SOS_RW_CMD> open` (remonte rw) à l'ouverture, `<SOS_RW_CMD> close`
  (sync + remonte ro) au `Drop` — **garanti sur tout chemin de retour, erreur
  comprise**. Branché autour du téléchargement de tuiles (`/api/admin/map`) et de
  la purge des tuiles (retour aux valeurs d'usine), sérialisé par un `tiles_lock`
  (un seul écrivain). No-op si `SOS_RW_CMD` absent (support déjà rw : Debian).
  Helper `sos-rw` ajouté à l'image Alpine. +2 tests (ordre `open`→`close`, no-op
  sans commande). Avec l'instantané Redb durable, SOSDATA reste **ro en
  permanence** hors fenêtres admin ponctuelles.
- **Persistance « instantané durable » (`sos-storage`) — modèle SOSDATA en lecture
  seule.** Nouveau `Store::open_durable(working, target, commit_cmd)` : la base
  Redb de **travail** vit sur un support inscriptible (tmpfs/RAM sur l'appliance
  Alpine *diskless*) et son contenu cohérent est recopié vers un **instantané
  durable** (`target`, sur la partition SOSDATA montée ro) après **chaque**
  écriture — via une **commande privilégiée** (`commit_cmd` : remonte rw → copie
  atomique → re-verrouille ro) ou, à défaut, par copie atomique en place. Au
  démarrage, l'instantané est **restauré** dans la base de travail (config,
  identité Ed25519, secrets retrouvés sans reconfiguration). Câblé dans
  `apps/sos-guide` par les variables `SOS_DB` (travail), `SOS_DB_DURABLE`
  (instantané) et `SOS_COMMIT_CMD` (helper). **Rétrocompatible** : sans
  `SOS_DB_DURABLE`, comportement historique inchangé (déploiement Debian). +3
  tests (restauration après reboot simulé, commande de commit externe, échec
  remonté). Validé bout-en-bout : l'identité du nœud survit à la perte du tmpfs.
- **Image Alpine Linux *diskless* (`image-alpine/`)** : `.img` bootable et
  reproductible (Pi 4, OS-en-RAM) embarquant l'app (binaire Rust **statique
  aarch64-musl** + web 29 langues) via apkovl ; partitions `SOSBOOT` (FAT32) +
  `SOSDATA` (ext4 **ro**, instantané durable + tuiles) ; service OpenRC
  supervisé ; helper `sos-commit-db` (fenêtre rw en ms). Build rootless dans
  conteneur `alpine:3.21` épinglé (`mtools`/`mke2fs -d`/`sfdisk`). Cross-build
  musl depuis x86_64 sans gcc (link `rust-lld` self-contained, zéro dep C).
- **Tableau de bord admin (`admin.html`) traduisible, sélecteur de langue + bascule
  temps réel.** Nouveau dictionnaire `web/data/admin-i18n.json` (203 clés UI) et
  recâblage complet d'`admin.html` par `data-i18n` / `data-i18n-ph` + `t()` +
  `applyI18n()` (mêmes principes que `install.html`) : ajout d'un **sélecteur de
  langue** dans la barre supérieure (29 langues) qui traduit toute la page —
  connexion, héros, 11 sections, statuts/badges/toasts dynamiques — gère le **RTL**
  et **se replie sur le FR**. Langue admin mémorisée (`sos_admin_lang`) ; sans
  préférence, suit `config.defaultLang` du nœud. **Couverture complète : les 29
  langues sont fournies** (203 clés chacune ; `_meta.todo_langs` vidé). _FR =
  source relue ; les 28 autres = traductions **automatiques** (chrome UI
  uniquement) à FAIRE RELIRE. Placeholders `{n}` `{z}` et le token littéral
  `REINITIALISER` préservés dans chaque langue._
- **Assistant d'installation (`install.html`) entièrement traduisible, bascule en
  temps réel.** Nouveau dictionnaire `web/data/install-i18n.json` (~80 clés UI ×
  **29 langues**) chargé une fois ; `install.html` recâblé par attributs
  `data-i18n` / `data-i18n-ph` + helper `t()` + `applyI18n()` : cliquer une langue
  traduit **immédiatement** toute la page (titres d'étapes, libellés, options,
  boutons, validations, aperçu carte, écran final), gère le sens d'écriture **RTL**
  (ar/fa/he) et **se replie sur le FR** pour toute clé manquante. La langue choisie
  reste le **défaut du portail `/`** (`config.defaultLang`, déjà consommé par
  `index.html`). _FR = source relue ; les 28 autres langues sont des traductions
  **automatiques** (UI uniquement, jamais les consignes vitales) — à FAIRE RELIRE
  par un humain avant diffusion (cf. `_meta.review` du dictionnaire)._

### Corrigé
- **Service systemd : démarrage sur installation neuve (`StateDirectory=sos-guide`).**
  `ProtectSystem=strict` + `ReadWritePaths=/var/lib/sos-guide` échouait avec
  `status=226/NAMESPACE` quand `/var/lib/sos-guide` n'existait pas encore (Pi
  fraîchement flashé). `StateDirectory=sos-guide` crée le répertoire d'état avec les
  bons droits **avant** le démarrage → l'unité est désormais autonome (indispensable
  pour l'image `.img` distribuable).

### Modifié
- **Port d'écoute par défaut : `8080` → `80`.** Le défaut de `SOS_LISTEN` dans
  `apps/sos-guide/src/main.rs` (et la doc du champ `PortalConfig::listen`) passe à
  `0.0.0.0:80` : le nœud sert la page sur le port standard sans variable d'env. La
  prod (systemd `SOS_LISTEN=0.0.0.0:80`) est inchangée ; le dev (`just run`/`net-sim`,
  `:18080`) reste sur son port non privilégié.
- **Portail : capture totale des chemins inconnus.** Tout chemin absent du webroot
  est désormais **redirigé (307) vers `/`** au lieu de renvoyer un 404 sec
  (`ServeDir::fallback` dans `crates/portal/src/lib.rs` — `not_found_service`
  écraserait le statut en 404 via `SetStatus`) — comportement
  attendu d'une borne captive : un visiteur qui tape une URL quelconque retombe sur
  le portail. Les fichiers réellement présents et les routes explicites
  (`/install`, `/admin`, `/api/*`, sondes captives, `/tiles`) restent servis tels quels.
- **Admin : retrait du widget de taille du texte (A−/100%/A+).** Le `<body>` d'`admin.html`
  porte désormais `data-no-a11y-zoom` ; `lib/sos-theme.js` respecte cet opt-out et
  n'injecte plus le widget flottant sur la page de gestion (la préférence de taille
  globale reste appliquée). Le portail public (`index.html`) et l'assistant
  (`install.html`) conservent le contrôle d'accessibilité.

### Supprimé
- **Portail : sections « Nœuds actifs » et « État du réseau » retirées** (`web/index.html`).
  Le titre « 🗺️ Carte de détresse » est retiré mais **la carte (fonction) est conservée**.
  Les blocs « 👥 Nœuds actifs » (liste `#carteNodes` + `renderNodes()`) et
  « 📶 État du réseau » (LEDs LoRa/Tor/WiFi + sondage `/api/status` `pollStatus`/`setLed`)
  sont supprimés (HTML + JS + CSS mort associé `.carte-node*`/`.net-led*`/`.led*`).

### Ajouté
- **Outillage d'image appareil (Phase 6 — rootfs RO + `.img` reproductible).**
  Nouveau dossier `sosguide/image/` : `sosguide-harden.sh` (minimisation/durcissement
  **idempotent** d'une Raspberry Pi OS Lite — purge Bluetooth/audio/mDNS/cups/rsyslog/
  swap, désactive timers `apt-daily` + radio BT, `swappiness=0`, boot sobre, LED
  d'activité éteinte ; épargne SSH/WiFi/`wpasupplicant`), `sosguide-readonly.sh`
  (rootfs lecture-seule `overlayroot` tmpfs résistant aux coupures, **garde-fou**
  exigeant `/var/lib/sos-guide` sur montage séparé pour ne pas perdre l'identité du
  nœud), et `README.md` (mode opératoire reproductible flash→durcir→partition de
  données→rootfs RO→capture `dd`+`pishrink`→**BOM du kit matériel**). Recettes
  `just image-harden`/`image-readonly` à **cible explicite** (jamais la prod).
  _Production du `.img` = à exécuter sur une SD/un Pi de build (matériel)._
- **Firmware `esp32-lora` initialisé (nœud satellite LoRa, Phase 4).** Cible
  ESP32-C3 (RISC-V, toolchain Rust standard), projet `no_std` en **workspace
  séparé** : init `esp-hal` 1.x, LED de vie, et `build_frame()` produisant la
  trame d'alerte au **format v2.5** (`heapless`, interop `sos-core`). **Vérifié
  par cross-compilation** (ELF 126 Ko, sans matériel). _Embassy, pilote SX127x et
  boucle mesh différés au flash sur matériel réel._
- **Splash / mode survie.** Le splash (numéros d'urgence visibles d'emblée) est
  conservé ; une fois « Entré », les visites suivantes vont **directement au
  contenu** (accès rapide guide/carte), le splash restant joignable via le logo.
- **Barre du bas d'accès rapide (accueil).** Barre fixe à 5 onglets (Carte ·
  Urgences · Guide · Outils · Menu), grandes zones tactiles, `safe-area-inset-bottom`,
  thémée, affichée seulement sur la page principale ; onglets → défilement vers
  la section, « Menu » → tiroir.
- **Accessibilité : zoom de texte (A−/A+).** Réglage de taille du texte sur 4
  niveaux (petit/normal/grand/très grand), persistant et appliqué avant peinture,
  via le module partagé `lib/sos-theme.js` ; contrôle injecté sur toutes les pages
  (bas-droite, `aria-live`). Cible WCAG 1.4.4.
- **Style & UX : gabarit CSS commun + thème clair/sombre unifié et persistant.**
  Le thème est factorisé dans `web/lib/sos-theme.js` (clé localStorage commune,
  application avant peinture, toggle de nav câblé ou bouton flottant injecté) et
  chargé par toutes les pages (`/`, `/install`, `/admin`, `/privacy`). Conséquence :
  un **bouton de bascule de thème sur toutes les pages** (avant : seulement
  l'accueil) et une persistance cohérente. Les tokens restent dans `sos-theme.css`
  (aucune page ne redéfinit `:root`).
- **Phase 5/6 logicielle : MAJ OS, confidentialité, durcissement, logs volatils.**
  `just os-update` (mise à niveau `apt` hors-bande, sudo). Page `/privacy.html`
  (PRIVACY.md rendue, thémée) + mention « Aucune donnée personnelle collectée » et
  lien dans le tiroir. **Durcissement systemd** du service (`ProtectSystem=strict`
  + `ReadWritePaths`, `PrivateTmp`, `ProtectKernel*`, `RestrictAddressFamilies`,
  `LockPersonality`…). **Logs volatils** en RAM (drop-in journald `Storage=volatile`,
  conformité nLPD). Supervision interne typée (tâches `tokio`) actée — zéro `sudo`
  depuis le web.

### Corrigé
- **Footer du tiroir : la mention/le lien de confidentialité n'est plus écrasé**
  au changement de langue (footer scindé : bloc privacy permanent + texte traduit
  `#drawerFooterText`).
- **Zoom de texte (A−/A+)** : masqué sur le splash (où il était inopérant et mal
  placé) et **remonté au-dessus de la barre du bas** en page principale (il y était
  caché). Barre du bas stabilisée (`translateZ(0)`) contre le dédoublement mobile.
- **Page d'alerte (SOS)** : ajout d'un **bouton de lecture vocale** « 🔊 Lire
  l'alerte » (synthèse vocale multilingue, lit cause + consignes ; s'arrête à la
  levée de l'alerte).
- **Accueil** : retrait du texte d'aide redondant sous la carte de détresse.
- **`/admin`** : bouton de déconnexion réduit + **icône seule** (🚪, sans texte).
- **Animations retirées** sur tout le portail (interface instantanée, plus sobre).
- **Zoom A−/A+** déplacé dans la **barre du haut** (à côté de langue/thème) ;
  l'onglet « Menu » (hamburger) **retiré de la barre du bas** (le tiroir s'ouvre
  depuis le hamburger du haut), barre du bas à 4 onglets.
- **Accueil** : la **carte de détresse est désormais en premier**, juste après le
  titre et le texte de bienvenue (les infos du lieu passent après la carte).
- **`/admin`** : bouton **thème clair/sombre** déplacé dans la barre du haut, **à
  côté de la déconnexion** ; les deux en petites icônes carrées (32 px).
- **Carte de détresse** : le clic sur la carte n'émet plus de ping ; un **seul
  ping** (au centre du nœud) déclenché **uniquement par le bouton** « Envoyer un
  ping de détresse » (curseur de la carte remis par défaut).

### Modifié
- **Nettoyage : état réseau honnête et données réelles (plus de simulation
  affichée).** L'accueil n'affiche plus de **nœuds simulés** ni de LEDs figées
  « Non actif · Phase 3/4 » : les LEDs WiFi/LoRa/Tor reflètent le **mode réel**
  (`/api/status` → inactif/simulation/actif) et seuls la borne, sa portée et les
  pings de détresse **réels** sont tracés. `/api/status` expose `activeNodes` /
  `meshPeers` (pairs réellement entendus, vide tant qu'aucun). `/admin` affiche
  les **nœuds actifs (maillage)** dans la supervision et l'état réseau honnête ;
  wording « Phase 3/4 » retiré (sources officielles, autorités, WiFi/LoRa).

### Ajouté
- **Manifeste de version signé + vérification (Phase 5).** Intégrité et
  authenticité des mises à jour : `sos-core::VersionManifest` (version + SHA-256 +
  date, charge canonique), signature **Ed25519 détachée** (`sos-security::
  sign_detached`/`verify_detached`, clé de publication distincte de l'identité du
  nœud), et `sos-cli sign-update`/`verify-update` (vérifie empreinte **puis**
  signature). Recette `just verify-release` à lancer avant `just update`. Vérifié
  avec des clés openssl ed25519 (interop PEM) : binaire altéré ou signature
  invalide ⇒ rejet (exit 1).
- **Supervision journald + mise à jour binaire atomique (Phase 5).** `just journal`
  capture les avertissements/erreurs runtime du service (`journalctl -p warning`)
  dans l'inbox d'`ERRORS.md` (curation par `just errors`). `just update` fait une
  **MAJ binaire atomique avec rollback** : sauvegarde `.prev`, installe par rename
  atomique, redémarre, vérifie `/api/status`, et **restaure automatiquement** la
  version précédente si la santé échoue.
- **`sos-cli` — sonde de santé et chien de garde matériel (Phase 5).** La crate
  `cli` devient bibliothèque + binaire. `sos-cli health` imprime les vitaux du
  nœud en JSON (température, mémoire, charge, disque) — lecture `/sys`+`/proc`+`df`,
  analyse pure testée, champs illisibles à `null` ; recette `just vitals`.
  `sos-cli watchdog` caresse `/dev/watchdog` (`bcm2835_wdt`) tant qu'une sonde
  applicative TCP juge le démon sain, et le désarme proprement à l'arrêt. **+9
  tests**, vérifié sur le Pi. _Unité systemd du watchdog à brancher au déploiement._
  Les vitaux sont aussi exposés à l'admin : `GET /api/admin/vitals` (collecte sur
  le pool bloquant) + **panneau « État du nœud » de `/admin`** affichant
  température, mémoire, disque et charge en direct.
- **`/admin` : statut honnête des sous-systèmes + panneau des nœuds de confiance.**
  Le panneau réseau (Tor · WiFi · LoRa) n'affiche plus des libellés figés
  « Phase 3/4 » mais le **mode réel** de chaque sous-système (`désactivé` /
  `simulation` / `actif`), exposé par `/api/status` (`subsystems`) depuis
  `SOS_NET_MODE`/`SOS_RADIO_MODE`/`SOS_GW_MODE`. Nouveau panneau **« Nœuds de
  confiance »** : liste les pairs connus et permet de remplacer/vider le registre
  (`trusted_nodes.json`) — l'UI manquante de la gestion admin du registre.
- **`sos-gateway` — service caché Tor v3 à surface restreinte (code, démon différé).**
  Sur le 3ᵉ canal (Tor, longue distance), le nœud n'expose qu'un **manifeste**
  minuscule (`{service, nodeId, version, phase, alertActive}`) servi sur une
  adresse loopback dédiée — **jamais** le portail, l'admin, ni la configuration
  (test garde-fou). `torrc` v3 généré (`HiddenServicePort` → loopback, `SocksPort 0`).
  Orchestrateur gaté `SOS_GW_MODE` (`off` par défaut / `simulate` sert le
  manifeste en HTTP sans Tor / `live` génère le torrc, démon `tor` différé). Le
  manifeste reflète l'état runtime en direct via le canal `watch` partagé.
  **+8 tests**, build natif Pi OK, simulate validé (phase qui bascule à l'install).
- **Gestion admin du registre des nœuds de confiance + persistance Redb.**
  Le registre des pairs (`trusted_nodes.json`) devient administrable et durable :
  table Redb `trusted`, API `GET/POST/DELETE /api/admin/trusted` (auth admin) avec
  **rechargement à chaud** du trousseau partagé (la radio voit les nouveaux pairs
  sans redémarrage). Au démarrage, Redb fait foi puis le fichier v2.5 est importé.
  **Correction** : la rotation de la clé du nœud (`regenerate_node_key`) préserve
  désormais le registre (auparavant perdu jusqu'au redémarrage). _UI `/admin` à
  venir ; l'API est pilotable au curl._
- **Registre des nœuds de confiance chargé au démarrage.** `apps/sos-guide` lit
  `SOS_TRUSTED_NODES` (défaut `/etc/sos-guide/trusted_nodes.json`, format v2.5) et
  charge les clés publiques des pairs dans le trousseau. **Débloque le maillage
  multi-nœuds** : sans registre, le nœud rejette toute alerte d'un autre nœud
  (`Untrusted`) ; avec, les alertes signées par les pairs déclarés sont vérifiées,
  admises et relayées. Fichier absent/illisible = non fatal. _À suivre : gestion
  admin du registre + préservation lors d'une rotation de la clé du nœud._
- **`sos-radio` — transport et relais mesh LoRa (code, matériel différé).** La
  crate `radio` quitte l'état de stub. Le codec de trame (JSON v2.5 compatible,
  signature Ed25519) vit déjà dans `sos-core` ; `sos-radio` ajoute :
  - `relay::evaluate` (pur, testé) : décode → **vérifie la signature** (registre
    de confiance) → admet (dédup + anti-rejeu) → relaie (`hop++` sous plafond).
    Une trame non signée ou d'un nœud inconnu est **jetée** (jamais affichée,
    jamais relayée).
  - `link` : abstraction `RadioLink` scindable (émetteur clonable + récepteur),
    avec `SimLink` en mémoire pour les tests et le mode `simulate`.
  - `device` : pilote série/SPI réel (SX1276 / Meshtastic T-Beam) **différé** —
    aucun matériel n'est branché ; `open` échoue proprement, `live` = no-op.
  - orchestrateur `run()` gaté par `SOS_RADIO_MODE` (`off` par défaut).
  **Câblage portail↔radio** : `inbox` et `keyring` de `NodeState` deviennent des
  `Arc` partagés (la radio vérifie les signatures, suit les rotations de clé, et
  admet les alertes mesh affichées par le portail) ; `NodeState.radio_tx` permet
  à `POST /api/alerts` de diffuser la trame signée sur le maillage (non bloquant).
  **+18 tests**, `just check` vert, build natif Pi OK, simulate validé (publication
  → trame diffusée). _Limite : sans chargement du registre de confiance au
  démarrage, seul le nœud lui-même est de confiance (item suivant)._

- **Phase 3 — réseau local `sos-network` (code complet, activation différée).**
  Réseau souverain hors-Internet, **gaté par `SOS_NET_MODE`** (`off` par défaut :
  aucun socket, aucune mutation système — `wlan0` est la ligne SSH du Pi, l'AP
  réelle reste désactivée pour éviter le lockout). Trois modes : `off` (no-op),
  `simulate` (DNS + DHCP sur binds loopback, sans mutation système, pour test
  hors-ligne), `live` (interface + `iptables` + `hostapd` ; présent mais non
  exécuté sur ce Pi). Modules (générateurs/codecs **purs et testés**) :
  - `plan` : décision d'AP — provisioning → `SOS-SETUP-XXXX` ouvert · urgence
    sans alerte → `SOS-GUIDE` **WPA2** · urgence + alerte → `SOS-GUIDE` **ouvert**
    (aucune barrière en détresse) ; repli sûr vers le SSID de config si la clé
    manque.
  - `dns` : serveur DNS de portail captif **fait main** (zéro dep C) — toute
    requête `A` → IP du nœud ; `AAAA` → réponse vide (IPv6 désactivée).
  - `dhcp` : serveur DHCPv4 **fait main**, **sans baux persistés** (pool
    10.0.0.10–250 en mémoire, conformité nLPD).
  - `hostapd` : génération de la conf (ouvert vs WPA2) + (re)démarrage gaté `live`.
  - `firewall` : règles `iptables` (`FORWARD DROP`, hashlimit ~30 req/s par IP,
    DoT/853 bloqué).
  - `iface` : `ip addr/link` (10.0.0.1/24) + `sysctl` IPv6 off.
  **Transition à chaud** sans reboot via un canal `watch<RuntimeSignal>`
  portail → réseau : l'AP bascule protégé↔ouvert selon l'état d'alerte, et
  provisioning↔urgence selon l'installation. `WIFI_SSID` déplacé dans `sos-core`
  (source unique partagée `portal`/`network`). Câblage app (`SOS_NET_MODE`,
  clé WiFi lue depuis la config persistée), outillage (`just net-sim`, et
  `just wifi-on`/`wifi-off` **différées** derrière le garde-fou `SOSGUIDE_GO_LIVE`),
  service systemd (`SOS_NET_MODE=off`). **+32 tests** (`sos-network`), `just check`
  vert, 0 warning. Go-live réel reporté à un accès SSH alternatif (eth0/dongle) +
  `hostapd`/`iptables` installés.

### Modifié
- **Workflow : passage au modèle DEV-SUR-PI (build natif), abandon de la
  cross-compilation musl.** On édite toujours sur le PC (dépôt git = source de
  vérité) mais on **synchronise** les sources vers le Raspberry Pi (`rsync`) et
  on **compile/exécute nativement** dessus (`aarch64-unknown-linux-gnu`, glibc,
  Rust 1.95 déjà présent). Le `justfile` est refondu : `just sync` (rsync PC→Pi),
  `just build`/`just pi` (build natif debug/release sur le Pi via SSH),
  `just run` (lance le nœud sur le Pi, `http://192.168.1.133:18080/`),
  `just install` (binaire `/usr/local/bin` + webroot + service systemd, depuis le
  build natif du Pi) ; `just check` reste local. Les réglages cross-compile
  (`rust-toolchain.toml`, `.cargo/config.toml`) ne sont plus synchronisés. La
  contrainte « binaire statique musl, zéro dep C » est **levée** ; les choix
  pur-Rust déjà livrés (tuiles via `curl`, QR `qrcodegen`) sont conservés en
  l'état. Premier build natif validé (3 min 20 s, nœud démarré en provisioning).
- **Carte de détresse et carte du lieu fusionnées en une seule carte.** Les
  tuiles OpenStreetMap servent désormais de **fond** à la carte de détresse de
  l'accueil ; par-dessus, un **cercle de portée WiFi ~30 m** géo-référencé (calé
  sur la latitude et le zoom des tuiles), la borne au centre, les pings de
  détresse anonymes et les nœuds simulés. Ajout d'un **mode plein écran**. La
  mosaïque de tuiles autonome est retirée. Le téléchargement des tuiles passe au
  **zoom 18** (~750 m de côté) pour rendre le cercle de 30 m nettement visible —
  re-télécharger la carte depuis `/admin` après mise à jour.
- **Page `/audit` fusionnée dans `/admin`.** La page publique `/audit` est
  retirée ; son contenu (« État du nœud — temps réel » via `/api/status` +
  « Conformité & principes ») est déplacé dans `/admin`, juste après le bloc
  d'alerte (section auth admin). La route `/audit` et le handler sont supprimés ;
  `/api/status` est conservé.
- **WiFi : protégé en veille, ouvert en alerte.** Décision produit : la borne
  ne doit pas servir de point d'accès gratuit au quotidien, mais aucun citoyen
  en détresse ne doit être bloqué par un mot de passe en urgence. Le nœud
  **génère** désormais une clé WiFi à l'installation (`sos-security::token`,
  alphabet lisible sans glyphes ambigus, unique par borne), **affichée à l'admin
  pour l'affiche papier** : le wizard `/install` la montre en fin de parcours
  (bouton imprimer), `/admin` l'affiche (projection admin, jamais publique) et
  permet de la **régénérer** (`regenerateWifiKey`). Le champ de saisie manuelle
  du mot de passe WiFi est retiré (`/install`, `/admin`). *La bascule réelle de
  l'AP (WPA en veille ↔ ouvert quand une alerte est active) relève de
  `sos-network` — Phase 3.* Spec mise à jour (CLAUDE.md, `STATE_EMERGENCY`).

### Sécurité
- **Durcissement des entrées du portail** (Phase 2). **Anti-XSS** : toute
  chaîne de configuration affichée (nom du lieu, réassurance…), les consignes
  d'alerte, les bulletins officiels et le message d'alerte publique sont validés
  à l'écriture — `<`/`>` et caractères de contrôle refusés (422), longueurs
  bornées, profondeur du document limitée. Primitive
  `sos-security::validate_text`.
- **CSRF** : les API mutantes (`/api/install`, `/api/admin/*`) vérifient que
  l'en-tête `Origin`, s'il est présent, correspond au `Host` (403 sinon) ; les
  clients non-navigateur sans `Origin` passent.
- **Rate-limit de l'auth admin** : tarpit à délai croissant (1 s→2 s→4 s,
  plafonné) après 5 échecs ; un mot de passe correct réussit immédiatement et
  remet le compteur à zéro — **l'admin n'est jamais verrouillé** (fiabilité >
  sécurité). +9 tests (59 au total).

### Ajouté
- **Page de connexion administrateur + dashboard humanitaire.** L'auth Basic
  native du navigateur laisse place à une **session par cookie** (`HttpOnly`,
  `SameSite=Strict`, expiration 8 h) ouverte via un **formulaire de login**
  stylé (`POST /api/admin/login` / `/logout`, sonde `/api/admin/session`).
  `require_admin` accepte cookie **ou** Basic (compat outils). `/admin` est
  refondue en tableau de bord sectionné (barre supérieure, phase en direct,
  bannière de mission, cartes), sans changer les fonctionnalités.
- **Retour aux valeurs d'usine** (`/admin`, zone danger, double confirmation) :
  `POST /api/admin/reset` efface configuration, mot de passe admin, alerte,
  bulletins, tuiles et sessions ; la borne repasse en mode installation.
  L'identité cryptographique du nœud est conservée.
- **Saisie GPS en degrés-minutes-secondes** : le champ coordonnées de `/admin`
  accepte `48°39'14.7"N 2°20'56.1"E` (copié de Google Maps) ou un couple
  décimal, et le convertit automatiquement en latitude/longitude.
- **Carte du lieu hors-ligne (tuiles OpenStreetMap)** : le nœud télécharge à
  l'installation une grille de tuiles OSM (5×5 au zoom 16) autour de son GPS et
  les sert **hors-ligne** en `/tiles`. Le téléchargement passe par le **`curl`
  système** (le TLS reste hors du binaire, qui demeure un statique aarch64-musl
  **pur Rust**). Endpoint `POST /api/admin/map` (auth admin), déclenché
  automatiquement en fin de wizard `/install` et re-déclenchable en `/admin`.
  L'accueil affiche une **mosaïque centrée sur le nœud** avec marqueur et
  attribution © OpenStreetMap (aucune lib JS, CSP `'self'`), avec repli sur le
  schéma SVG si le nœud était hors-ligne. Nouvelle variable `SOS_TILES_DIR`
  (défaut `/var/lib/sos-guide/tiles`). +1 test (65 au total).
- **QR Code WiFi pour l'affiche du lieu** : le nœud génère un QR de **jonction
  WiFi** (`WIFI:S:SOS-GUIDE;T:WPA;P:<clé>;;`) en **SVG autonome** (modules noirs
  sur fond blanc, contraste fixe), encodé côté Rust par `qrcodegen` (port pur,
  zéro dépendance, hors-ligne). À l'install, le panneau de fin affiche le QR à
  côté de la clé (**scanner = rejoindre le réseau**, à imprimer). En `/admin`,
  `GET /api/admin/wifi-qr` (**auth admin** : le QR encode la clé secrète) affiche
  le QR dans le panneau réseau, avec un bouton « Réimprimer » et un
  rafraîchissement automatique après régénération de la clé. SSID figé en
  constante partagée avec la future Phase 3 (hostapd). +2 tests (64 au total).
- **Carte d'accueil personnalisée par type de lieu** : nouveau champ `venueType`
  (mairie · école · commerce · tabac/presse · dispensaire · lieu public · autre)
  choisi à l'installation et éditable en `/admin` — adapte l'icône et affiche un
  badge de type sur la carte de bienvenue. Champ **`sponsor`** optionnel
  (commerce/hébergeur de la borne) crédité discrètement sur l'accueil
  (« Borne mise à disposition par … »). Tous deux exposés dans la config publique
  (non sensibles). Ouvre un modèle d'adoption/financement par **sponsoring local**.
- **Outil Morse** (page d'accueil) : saisie → code Morse international en direct,
  **émission lumineuse plein écran** (balise blanc/noir) et **sonore** (Web Audio,
  620 Hz, timing standard point/trait/espaces), tableau de référence repliable.
  Entièrement hors-ligne, sans dépendance.
- **État du réseau** (page d'accueil) : section avec **LED LoRa / Tor / WiFi**
  affichées **rouge « non actif · Phase 3/4 »** — statut honnête tant que
  `sos-network`/`sos-radio`/`sos-gateway` ne sont pas livrés.
- **Transmission aux autorités** (`/admin`) : destinataires institutionnels
  (nom · canal Ethernet/Tor · adresse · pays) éditables et persistés dans la
  configuration. Statut honnête « aucun canal de sortie · Phase 3/4 » + compteur
  « N destinataire(s) en attente de relais » quand une alerte est active ; le
  relais effectif interviendra dès qu'un canal de sortie existera.
- **Garantie vie privée de l'alerte** : test `active_alert_carries_no_personal_data`
  qui **fige** le contrat — une alerte ne porte que cause, consignes et
  horodatage, jamais d'identité de citoyen ni de donnée personnelle.
- **Sources officielles & ingestion (cache hors-ligne)** : nouveau modèle de
  domaine `sos-core::official` — `OfficialBulletin` (source, catégorie typée
  `OfficialCategory` : météo / catastrophe / gouvernement / sanitaire / autre,
  pays, titre, corps, dates de publication et d'ingestion, lien optionnel) et
  `OfficialCache` (déduplication par source+titre, tri du plus récent au plus
  ancien, plafond de 50 bulletins, filtrage par pays + bulletins globaux). Champs
  bornés en longueur. Persistance Redb (table `official`, `save/load/clear_official`,
  survit au redémarrage). API portail : `GET /api/official` (public, filtré sur
  le `countryCode` du nœud + bulletins de portée globale, libellés de catégorie)
  et `POST`/`DELETE /api/admin/official` (auth admin). L'**import manuel** est le
  repli d'acquisition toujours disponible sans réseau ; la **récupération
  automatique** sur canal de sortie (Ethernet de maintenance / Tor) réutilisera la
  même persistance une fois la connectivité disponible (`gateway`/`network`,
  Phase 3/4). Frontend : les bulletins en cache s'affichent **sous les consignes
  locales** sur la page SOS plein écran (titre de section traduit, 7 langues
  vérifiées, repli français ; contenu officiel jamais traduit) ; panneau
  d'import + liste + purge dans `/admin`, avec statut honnête « récupération auto
  à la connectivité · Phase 3/4 ».
- **Page SOS multilingue** : helper i18n partagé `web/lib/sos-i18n.js` (libellés
  des causes d'alerte par `AlertType` + consignes génériques). La bascule SOS
  plein écran s'affiche dans la langue du citoyen (choix explicite > `defaultLang`
  du nœud > navigateur), avec **repli français**. 7 langues vérifiées (fr, en,
  de, it, es, pt, nl) ; les autres retombent sur le français en attendant une
  relecture humaine (texte de sécurité — jamais de traduction automatique). Les
  consignes **locales** saisies par l'admin restent affichées telles quelles.
- **`/install` — choix de la langue et du pays** : 1ère étape « Langue » (grille
  des 29 langues, persistée en `defaultLang`) ; le **pays se déduit de la
  langue** (liste ordonnée, défaut automatique) et **adapte l'interface** —
  préremplissage des numéros d'urgence selon le pays (France 15/17/18, Suisse
  144/117/118, repli 112), `countryCode` persisté. Un numéro saisi à la main
  n'est jamais écrasé. Le champ « Pays » (texte libre) devient un menu déroulant.
- **Carte de détresse interactive** (accueil, `web/index.html`) : canvas sans
  dépendance ni tuile (nœud SOS au centre, cercle de portée, nœuds distants
  colorés par réseau WiFi/LoRa/Tor, zoom), **ping de détresse anonyme** (aucune
  donnée personnelle) et **tableau des nœuds actifs**. Frontend à données
  simulées — branché sur le maillage réel en Phases 3-4. Thème clair/sombre.
- **Mode alerte → page SOS plein écran** : l'administrateur émet une alerte
  depuis `/admin` (cause typée parmi les `AlertType` v2.5 + **consignes
  locales** propres au lieu) ; le portail des citoyens bascule alors en **page
  SOS plein écran sobre** (cause + consignes + gestes qui sauvent), par
  interrogation de `GET /api/alert`. `POST` / `DELETE /api/admin/alert` (auth
  admin) ; une cause `FIN_ALERTE` clôt l'alerte. Alerte **persistée en Redb**
  (`sos-storage`, survit au redémarrage), modélisée par `sos-core::ActiveAlert`
  (consignes ≤ 2000 caractères). Socle commun : l'ingestion automatique des
  sources officielles (à venir) écrira la même alerte. +5 tests.
- **Carte & position** (`/install` + `/admin`) : échelle configurable (aucune /
  ville / pays / monde), position GPS du nœud (**centre = émetteur SOS-GUIDE**),
  **aperçu radar hors-ligne** en SVG (sans dépendance ni tuile externe). Conçu
  pour un tableau de bord humanitaire collectif : seuls des **pings GPS d'alerte**
  y figureront (jamais de clients) — le rendu temps réel viendra avec le maillage.
- **`/admin` — panneau réseau du nœud** : Tor (`.onion`), point d'accès WiFi
  (canal éditable), LoRa (activation) avec **statut honnête « non actif
  (Phase 3/4) »** tant que `sos-network`/`sos-gateway`/`sos-radio` sont des squelettes.
- **`/install` — wizard multi-étapes** : 5 étapes (Lieu · Position & carte ·
  Contacts · Message & WiFi · Accès admin) avec barre de progression et
  validation par étape ; « lancé une seule fois » (redirige vers `/admin`).
- **`/admin` — rotation fine des secrets** (`POST /api/admin/secrets`) : mot de
  passe administrateur, mot de passe WiFi et/ou **régénération de la clé de
  signature Ed25519** du nœud, indépendamment.
- **Pages portail `/install`, `/admin`, `/audit`** (`sos-portal` + webroot) :
  - `/install` : wizard de configuration du lieu (nom, adresse, contacts,
    réassurance, mot de passe WiFi, **mot de passe administrateur**). Écrit la
    config et l'empreinte du mot de passe dans Redb, marque `installed: true`
    → bascule en phase **EMERGENCY**. Disponible une seule fois (409 ensuite).
  - `/admin` : administration locale protégée par **HTTP Basic** ; édition de la
    config (fusion non destructive) et rotation du mot de passe. Accessible
    seulement une fois le nœud installé.
  - `/audit` : état **public en lecture seule** du nœud (`/api/status` :
    identifiant, version, phase, nombre d'alertes) — aucune décision, aucun secret.
  - Pages gardées par le cycle de vie (redirections install ⇄ admin).
- `sos-security::password` : empreinte de mot de passe administrateur (sel
  aléatoire + **SHA-256 itéré 100 000×**, comparaison à temps constant).
- `sos-storage` : stockage de l'empreinte admin (`save/load_admin_password`).
- **Détection de portail captif multi-OS étendue** : Apple, Android/Google,
  Windows (NCSI), Firefox, GNOME/NetworkManager, KDE, Kindle (chemins de sonde).
- **`sos-core::Lifecycle`** : cycle de vie du nœud `PROVISIONING` → `EMERGENCY`,
  transition à **sens unique** (jamais de retour arrière silencieux), déduit du
  champ `installed` de la config, noms de fil interop v2.5
  (`STATE_PROVISIONING`/`STATE_EMERGENCY`). Phase déterminée et journalisée au
  démarrage. 3 tests.
- `sos-storage` : `config_installed()` (déduction de la phase) et **branchement
  de la projection publique** : `/data/config.json` est désormais servi depuis
  Redb (`store.public_config`) ; repli sur la `config.json` livrée tant que le
  nœud n'est pas provisionné. Jamais le fichier brut.
- **`sos-storage` (Redb)** : persistance locale tolérante aux coupures
  (transactions ACID, un seul fichier, zéro dépendance externe). Stocke
  l'**identité du nœud** (clé Ed25519 PEM) et la **configuration**, expose la
  **projection publique** `public_config` (configuration sans `wifiPassword`,
  seule forme servable au portail). 3 tests (round-trip + persistance après
  réouverture, projection, rejet JSON invalide).
- `apps/sos-guide` : l'identité du nœud est désormais **persistée** — résolution
  `SOS_PRIVATE_KEY_PEM` → Redb → génération+sauvegarde. Base ouvrable via
  `SOS_DB` (défaut `/var/lib/sos-guide/sos-guide.redb`). Un échec d'accès au
  disque n'est pas fatal : repli sur identité éphémère (mode dégradé).
  **Fin de l'identité perdue à chaque redémarrage.**
- **Page d'accueil utilisateurs** (`sosguide/web/`, servie par `sos-portal`) :
  splash avec numéros d'urgence + guide d'urgence. Récupérée de la v3 et
  modernisée :
  - **i18n 29 langues** avec repli sur le français (romanche inclus) ;
  - **synthèse vocale multilingue** (TTS via Web Speech API, hors-Internet) ;
  - **contenus médias** : texte, image, vidéo, audio (`safeMedia`, anti
    `javascript:`/traversée) ;
  - **outils d'urgence** : métronome RCP, minuterie (corrige l'action
    `openTimer` orpheline des JSON), flash SOS, lampe, alarme, sifflet,
    respiration ;
  - thème clair/sombre, CSP stricte, accessibilité WCAG.
  Déployée sur le Pi par `just deploy` / `just install` (`/var/www/sos-guide`).
- **Audit local** (`audit-control/`, `just audit`, généré par `audit-gen.py`) :
  page ouverte sur le PC de dev (Arch) — progression du projet, erreurs
  détectées, corrections, et **génération du prompt de la prochaine session**
  par cases à cocher. Lit `docs/ROADMAP.md` + `ERRORS.md`. Hors de l'img produit.
- Service systemd `configs/sos-guide.service` (`Restart=always`, journald),
  écoute sur le **port 80**, et recettes `just install` / `just logs`.
- `sosguide/rust-toolchain.toml` : toolchain stable épinglée + cible musl auto.

### Modifié
- **Numéros d'urgence = pays, jamais la langue** (sécurité) : la grille du splash
  (`buildUrgences`) tire désormais ses numéros **uniquement** de la config du nœud
  (fixée au pays à l'install) puis d'une **table pays** (`COUNTRY_EMERGENCY`,
  miroir du wizard) — plus jamais des fichiers de langue. Changer de langue ne
  modifie plus aucun numéro ; seuls les libellés se traduisent. Un numéro
  introuvable ⇒ carte non affichée (jamais de numéro potentiellement faux). Le
  champ `number` a été **retiré des `urgences` des 29 fichiers `data/*.json`**
  pour interdire structurellement toute divergence par langue.
- **Deux projections de configuration distinctes** (`sos-storage`) : `public_config`
  (servie au portail public) masque désormais **`wifiPassword` ET `authorities`** ;
  nouvelle `admin_config` (servie à `/api/admin/config`) ne masque que les secrets
  à rotation dédiée (`wifiPassword`), pour que l'admin puisse éditer les
  destinataires institutionnels sans les exposer publiquement.
- **`/admin` — contacts d'urgence locaux restylés** : champs présentés en
  cartes (une par contact) avec icône, `type=tel`/`inputmode=tel` et indication
  par défaut (15/17/18…), au lieu d'une simple grille d'`input`.
- **Réorganisation du dépôt** (en place dans `V1/`) : `docs/` (tous les `.md` de
  pilotage), `audit-control/` (outillage build + audit local + `ERRORS.md` +
  agents), `sosguide/` (le produit : workspace Rust + webroot). `CLAUDE.md`
  reste à la racine (contrat d'agent chargé à chaque session).
- `justfile` : `deploy`/`run`/`install` poussent le webroot (`sosguide/web/`)
  sur le Pi ; nouvelle recette `just audit`.
- `configs/sos-guide.service` : écoute sur le **port 80** (`SOS_LISTEN=0.0.0.0:80`).
- Cible de production : **Raspberry Pi OS Lite** (systemd), déploiement SSH/SCP,
  cross-compilation vers `aarch64-unknown-linux-musl` (binaire statique).

### Corrigé
- **Filtre pays des bulletins officiels inopérant** : `/api/official` lisait
  `countryCode` au **niveau racine** de la config, alors qu'il est stocké dans
  `establishment.countryCode` (écrit par le wizard) → le filtre était toujours
  vide et renvoyait **tous** les bulletins. Lecture corrigée (`node_country_code`
  lit `establishment.countryCode`) : un nœud `ML` ne voit plus que les bulletins
  `ML` + globaux, jamais ceux d'un autre pays. _(Régression introduite avec
  l'ingestion des sources officielles, jamais publiée.)_
- Pages `/install`, `/admin`, `/audit` : **couleurs incohérentes** (fond
  `var(--bg)` non défini → texte clair sur fond blanc, effet « inversé »).
  Ajout du fond et synchronisation du thème clair/sombre avec l'index.
- `/install`, `/admin` : **dimensions des champs** (largeur débordante) —
  ajout de `box-sizing:border-box` et hauteur de champ homogène.
- `audit-control/justfile` (ERR-001) : la **capture automatique d'erreurs**
  était inopérante — les recettes faisaient `cd` dans le dossier produit (sans
  justfile) avant d'appeler `just _capture` (« no justfile found »). Corrigé par
  une référence explicite `just --justfile "{{justfile()}}" _capture`.
- `apps/sos-guide` : webroot **optionnel** — le nœud démarre sur un Raspberry Pi
  vierge sans boucle de redémarrage (`webroot introuvable` n'est plus fatal).
- `web/PRIVACY.md` : réécrit pour l'architecture Rust (Axum sans logs d'accès,
  démon unique zéro `sudo` web, Ed25519, Tor v3, journald volatile, `overlayroot`)
  — suppression des mentions obsolètes nginx/htpasswd/AES-PBKDF2/timer-SHA256.

### Retiré
- **Gestion des médias dans `/admin`** (`GET`/`PUT`/`DELETE /api/admin/media`,
  handlers `media_*`, `safe_media_filename`, UI d'upload) : décision produit de
  retirer le téléversement de médias depuis le portail (réduction de la surface
  d'attaque côté admin et simplification). Le service statique des fichiers du
  webroot reste inchangé.
- Tableau de bord **intégré au produit** (`/dashboard`, `/api/health`,
  `/api/progress`, modules `health`/`progress` de `sos-portal`) : déplacé hors
  du produit vers l'audit local `audit-control/` pour garder l'**img propre**.
- Backend PHP de la v3 (`admin.php`, `*.php`) : non repris — remplacé par
  `sos-portal` / `sos-cli`.

## [0.1.0]

### Ajouté
- Squelette du workspace Rust : crates `core`, `radio`, `gateway`, `storage`,
  `network`, `portal`, `security`, `cli` et binaire `apps/sos-guide`.
- Domaine métier `sos-core` : `AlertPacket`, `AlertInbox`, états du cycle de vie.
- `sos-security` : trousseau de clés Ed25519 (`keyring`).
- `sos-portal` : base du portail captif (Axum).
- Profils de build et lints workspace (interdiction de `unwrap`/`panic`/`unsafe`…).
- Emplacement du firmware ESP32-LoRa (`no_std` + Embassy), workspace séparé.
