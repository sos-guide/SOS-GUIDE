# CLAUDE.md — SOS-GUIDE

> Spécification & blueprint d'ingénierie, pensé pour **tous les modèles Claude**.
> Lecture « à plusieurs niveaux de zoom » : on dézoome pour le projet entier,
> on zoome pour un élément, on zoome encore pour son détail. À chaque niveau,
> les mêmes 8 dimensions répondent à la même grille.

> **⚠️ Enjeu — vie ou mort, intérêt public.** SOS-GUIDE est un **équipement de
> survie**, consulté quand tout le reste a lâché : courant coupé, réseau mort,
> catastrophe, confinement, bunker. Une borne qui **ment, plante ou affiche un
> mauvais numéro peut coûter une vie**. Tout choix se juge à cette aune :
> **fiabilité avant tout**, contenu vital exact (jamais inventé/auto-traduit),
> **hors-ligne par construction**. Le projet est **gratuit, souverain et
> d'intérêt public** — sa valeur est de **sauver des gens, pas de capter des
> données** : zéro donnée personnelle, zéro dépendance à un tiers, **aucun
> Internet requis pour servir l'essentiel** (un nœud doit s'installer et secourir
> même seul, sans aucune infrastructure autour de lui).
>
> Cible produit : **Alpine Linux diskless + SOS-GUIDE** sur Raspberry Pi 4
> (OS-en-RAM, image `.img` reproductible — voir [`image-alpine/`](image-alpine)).

## Comment lire ce fichier

Trois niveaux de zoom, huit dimensions constantes.

| Zoom | Portée | Section |
|---|---|---|
| 🔭 **0 — Général** | Le projet entier | [Zoom 0](#-zoom-0--vue-générale) |
| 🗺️ **1 — Éléments** | La carte des crates | [Zoom 1](#️-zoom-1--carte-des-éléments) |
| 🔬 **2 — Détail** | Chaque crate, une par une | [Zoom 2](#-zoom-2--détail-de-chaque-élément) |

### Fichiers de pilotage

Les fichiers de pilotage (`docs/` pour la documentation, `audit-control/ERRORS.md` pour les erreurs, `CLAUDE.md` à la racine) :

| Fichier | Rôle | Tenu par |
|---|---|---|
| `CLAUDE.md` | Spécification, rôle agent, essentiel hérité | l'humain + l'agent |
| `ASK.md` | **Dialogue d'évolution** : questions ouvertes (humain↔agent) + journal des décisions | l'humain + l'agent |
| `README.md` | Présentation, build & déploiement | l'humain |
| `ROADMAP.md` | Jalons + journal d'étapes (après chaque correction) | l'agent |
| `CHANGELOG.md` | Journal des versions (SemVer) | l'agent |
| `ERRORS.md` | Registre central des erreurs (auto-alimenté + curé) | `just` + `error-curator` |

Les **8 dimensions** (la grille appliquée à chaque niveau) :

1. **Informations** — quelles données l'élément manipule.
2. **Fonction** — ce qu'il fait.
3. **Réponse** — format de sortie attendu (de l'agent comme du code).
4. **Langage** — Rust pour le code ; français pour l'agent.
5. **Agent** — le rôle/persona à endosser pour y travailler.
6. **Contexte** — contraintes invisibles dans le code.
7. **Code** — règles de codage applicables.
8. **QQOQCP** — Qui · Quoi · Où · Quand · Comment · Pourquoi · Combien.

---

## 🔭 Zoom 0 — Vue générale

### QQOQCP du projet

| | |
|---|---|
| **Qui** | Un nœud autonome, déployé par un administrateur local, pour des citoyens sans connectivité. **Intérêt public, gratuit.** |
| **Quoi** | Plateforme souveraine de communication d'urgence fonctionnant **hors-Internet**. |
| **Où** | Raspberry Pi 4 — **image Alpine Linux diskless** (OS-en-RAM) ; nœuds satellites ESP32 (LoRa) à venir. |
| **Quand** | Catastrophe naturelle, coupure réseau/électrique, cyberattaque, bunker — mode dégradé. |
| **Comment** | **AP WiFi toujours actif** (le nœud *est* le réseau) + portail captif ; **uplink Ethernet optionnel** (sorties : tuiles OSM, MAJ OTA, Tor, bulletins) ; maillage LoRa/Reticulum ; liens longue distance via Tor. |
| **Pourquoi** | Diffuser vite l'information vitale, préserver la confidentialité, **survivre aux coupures et fonctionner seul**. |
| **Combien** | Pi 2 Go RAM ; binaire statique aarch64-musl ; un seul processus, empreinte minimale, OS-en-RAM. |

### Informations

Identités Ed25519 par nœud, alertes signées (`AlertPacket`), configuration du
nœud, projection publique de cette config. Aucune donnée personnelle stockée
(conformité nLPD/RGPD). Secrets dans Redb, jamais dans Git ni les logs.

### Fonction

Deux états de cycle de vie (provisioning → urgence), trois canaux de
communication (WiFi local, LoRa, Tor), une supervision interne unique
remplaçant l'orchestration shell de la v2.5.

### Réponse (format de sortie de l'agent)

Réponse directe, sans phrase d'introduction ni récapitulatif non demandé. Code
en blocs balisés avec le langage. JSON pour les données structurées. Conclusion
en une phrase si nécessaire. Terminer quand la tâche est complète.

### Langage

Code : **Rust stable exclusivement**. Réponses & commentaires métier : français.
Commentaires de code : uniquement pour un *WHY* non évident, en anglais.

### Agent (rôle)

Architecte Rust senior **et** ingénieur systèmes embarqués Linux. Style
laconique, ton technique et direct. Expertise secondaire : optimisation
mémoire/CPU, réseau, fiabilité hors-ligne. Voir l'agent
[`audit-control/agents/rust-architect.md`](audit-control/agents/rust-architect.md).

Comportement : pour les choix mineurs (nommage, défauts), décide et note la
décision ; pour un changement de scope ou une action destructive, demande
confirmation. Lis le code existant avant de le modifier, signale les risques,
tiens le `CHANGELOG.md` à jour et **ajoute une étape datée dans `ROADMAP.md`
après chaque correction**. N'ajoute aucun refactoring/abstraction non
demandé : le plus simple qui fonctionne. Quand tu as de quoi agir, agis ;
avant d'affirmer qu'une chose fonctionne, vérifie-la sur un résultat concret.

### Contexte

Architecture hexagonale (Ports & Adapters), DDD léger : séparation stricte
Domaine / Application / Infrastructure / Interfaces, faible couplage, forte
testabilité. Réécriture Rust d'un legacy v2.5 (PHP/Bash/Python) — voir
[Hérité v2.5](#hérité-v25--à-préserver). **Ordre de priorité de toute
proposition : fiabilité > simplicité > sécurité > sobriété mémoire > performances.**
La fiabilité prime parce que **des vies en dépendent** : préférer toujours le
comportement le plus sûr et le plus prévisible, dégrader proprement (jamais de
panique, jamais d'invention de contenu vital), et ne **rien ajouter** qui élargit
la surface d'attaque ou de panne sans bénéfice vital clair (cf. [`ASK.md`](ASK.md)
pour les idées mises de côté — ex. chat embarqué, écarté car risque > utilité).

### Code (règles)

- **Interdits** (lints workspace) : `unwrap()`, `expect()`, `panic!()`,
  `todo!()`, `unimplemented!()`, `unsafe`. Toute erreur via `Result<T, E>`.
- **Mémoire** : préférer `&str`, `&[u8]`, `Cow`, `SmallVec`, `ArrayVec` ;
  éviter `clone()`, `String`/`Vec` inutiles ; justifier toute allocation.
- **Async** : Tokio uniquement (Mutex/RwLock/mpsc/broadcast/watch Tokio).
  Jamais de Mutex bloquant ni de blocage d'une tâche async.
- **Qualité** : compile sans warning, `cargo fmt` + `cargo clippy` propres,
  tests inclus, APIs publiques documentées (`missing_docs`).

---

## 🗺️ Zoom 1 — Carte des éléments

Workspace dans [`sosguide/`](sosguide). Le firmware ESP32 est un workspace **séparé**
(toolchain et cible différentes), exclu du workspace hôte.

| Crate | Couche | Rôle (Quoi) | Dépend de | État |
|---|---|---|---|---|
| [`core`](sosguide/crates/core) | Domaine | Entités, états du cycle de vie, règles, manifeste de version. Zéro infra. | — | **mûr** (testé) |
| [`radio`](sosguide/crates/radio) | Infra | LoRa : transport + relais mesh des alertes. | core, security | **code complet** (gaté `SOS_RADIO_MODE`, pilote matériel différé) |
| [`gateway`](sosguide/crates/gateway) | Infra | Tor v3 : manifeste `.onion` restreint. | core | **code complet** (gaté `SOS_GW_MODE`, démon tor différé) |
| [`storage`](sosguide/crates/storage) | Infra | Redb durable, **instantané SOSDATA ro** + fenêtres rw, faible RAM. | core | **mûr** (persistance durable + ro) |
| [`network`](sosguide/crates/network) | Infra | AP WiFi, DNS local, DHCP, isolation. | core | **code complet** (gaté `SOS_NET_MODE`, go-live = matériel) |
| [`portal`](sosguide/crates/portal) | Interface | Portail captif, web (Axum), `/install` + `/admin` i18n. | core | **mûr** |
| [`security`](sosguide/crates/security) | Infra | Ed25519, Tor v3, signatures de release, validation des entrées. | core | **mûr** |
| [`cli`](sosguide/crates/cli) | Interface | `sos-cli` : santé + watchdog + sign/verify + **MAJ OTA `update`**. | core, security | **mûr** (binaire) |
| [`apps/sos-guide`](sosguide/apps/sos-guide) | Application | Binaire principal : assemble et supervise. | toutes | **mûr** |
| [`firmware/esp32-lora`](sosguide/firmware/esp32-lora) | Embarqué | Nœud satellite `no_std` (ESP32-C3, workspace séparé). | — | initialisé (compile ; Embassy/SX127x différés) |

---

## 🔬 Zoom 2 — Détail de chaque élément

> Grille compacte par crate. Tout le code obéit aux mêmes dimensions Langage,
> Réponse, Agent et Code que le [Zoom 0](#-zoom-0--vue-générale) — on ne répète
> ici que ce qui est spécifique.

### `sos-core` — domaine métier

- **Informations** : `AlertPacket`, `AlertType`, `PROTOCOL_VERSION`, `AlertInbox`,
  `InboxAlert`, `Admission` ; états `STATE_PROVISIONING` / `STATE_EMERGENCY` ;
  `VersionManifest` (+ `sha256_hex`) pour l'intégrité des mises à jour.
- **Fonction** : types et règles métier purs (validation, anti-rejeu, admission,
  charge canonique du manifeste de version).
- **Contexte** : aucune dépendance d'infrastructure ; portage des données LoRa
  v2.5 (signature Ed25519 + anti-rejeu, modèle de données jugé sain).
- **QQOQCP** : *Pourquoi* — cœur testable, indépendant des adaptateurs.

### `sos-radio` — transport & relais mesh LoRa

- **Informations** : trames LoRa (format JSON compact hérité v2.5, codec dans
  `sos-core`), liens UART/SPI. **Canal réservé exclusivement aux alertes.**
- **Fonction** : transporter et **relayer** les `AlertPacket` signés.
  `relay::evaluate` (pur) : décode → **vérifie la signature** (registre de
  confiance `sos-security`) → admet (dédup + anti-rejeu `AlertInbox`) → rediffuse
  (`hop++` sous plafond). Une trame non signée / d'un nœud inconnu est **jetée**.
- **Modes (`SOS_RADIO_MODE`, défaut `off`)** : `off` = no-op ; `simulate` =
  transport en mémoire (`SimLink`), trames émises journalisées, sans matériel ;
  `live` = pilote série/SPI réel (`device`) — **différé**, aucun matériel LoRa
  branché (`open` échoue proprement, `live` retombe en no-op journalisé).
- **Contexte** : remplace `lora-service.py`. Cibles matérielles SX1276 (SPI) /
  Meshtastic T-Beam (USB) **différées**. *Combien* : encodage le plus compact prime.
- **Câblage** : `inbox` et `keyring` partagés (`Arc`) avec `sos-portal` ; le
  portail pousse les alertes publiées (`NodeState.radio_tx`, `POST /api/alerts`),
  la radio admet les alertes reçues dans la boîte affichée.
- **Confiance** : le registre des pairs (`trusted_nodes.json` v2.5) est persisté
  dans Redb (table `trusted`) et administrable à chaud (`GET/POST/DELETE
  /api/admin/trusted`) ; au démarrage Redb fait foi, sinon le fichier
  `SOS_TRUSTED_NODES` est importé. La rotation de la clé du nœud préserve le
  registre. Sans aucun registre, seul le nœud lui-même est de confiance.
  *À suivre : panneau UI dans `/admin` (l'API est pilotable au curl).*

### `sos-gateway` — liens longue distance

- **Fonction** : passerelle réseau local ↔ Tor ; modèle **3 canaux** (WiFi/LoRa/Tor).
- **Contexte** : surface `.onion` restreinte (identification + manifeste,
  `127.0.0.1` dédié) — jamais le portail ni l'admin, jamais la config complète.
- **Manifeste** (`manifest::build`, pur) : `{service, nodeId, version, phase,
  alertActive}` seulement ; reflété en direct depuis le canal `watch<RuntimeSignal>`
  du portail. `torrc::torrc` génère la conf du service caché **v3** (`SocksPort 0`,
  `HiddenServicePort 80` → manifeste loopback).
- **Modes (`SOS_GW_MODE`, défaut `off`)** : `off` = no-op ; `simulate` = sert le
  manifeste en HTTP loopback (axum), sans Tor ; `live` = génère le `torrc` puis
  démarre `tor` — **différé** (démon absent du Pi : le `torrc` est produit, le
  lancement est journalisé comme à faire).

### `sos-storage` — persistance

- **Informations** : config du nœud, identités, clés ; projection publique de la config.
- **Fonction** : Redb (zéro dépendance externe, tolérance aux coupures, faible RAM).
- **Contexte** : remplace `config.json` + écritures PHP. Une projection publique
  ≠ la config complète (la fuite `GET /data/config.json` de la v2.5 est corrigée).

### `sos-network` — réseau local

- **Fonction** : AP WiFi (via `hostapd`/nl80211), DNS + DHCP locaux en Rust pur, isolation netfilter.
- **Contexte** : DHCP sans baux persistés (en mémoire), IPv6 désactivé, isolation
  `FORWARD DROP` + hashlimit par IP, DoT/853 bloqué. *Combien* : ~30 req/s par IP.
- **Modes (`SOS_NET_MODE`, défaut `off`)** : `off` = no-op total (aucun socket,
  aucune mutation système) ; `simulate` = DNS + DHCP sur binds loopback/ports
  hauts, **sans toucher au système** (test hors-ligne) ; `live` = `simulate` +
  interface + `iptables` + `hostapd`. **Go-live différé** : sur le Pi de dev,
  `wlan0` est la ligne SSH → activer l'AP = lockout. Le code `live` existe mais
  n'est pas exécuté tant qu'un accès alternatif (eth0/dongle) + `hostapd`/
  `iptables` n'existent pas (garde-fou `SOSGUIDE_GO_LIVE` sur `just wifi-on`).
- **Décision/codecs purs** : `plan_for` (quel SSID, ouvert vs WPA2 selon
  alerte/installation), `dns`/`dhcp`/`hostapd`/`firewall`/`iface` sont des
  générateurs/codecs purs **entièrement testés** ; seuls les effets de bord
  (sockets, `tokio::process`) vivent dans l'orchestrateur.
- **Transition à chaud** : le portail émet un `RuntimeSignal` (`installed`,
  `alert_active`) sur un canal `tokio::sync::watch` ; le réseau recalcule le plan
  d'AP et bascule protégé↔ouvert sans reboot. `WIFI_SSID` vit dans `sos-core`
  (source unique partagée portal/network).

### `sos-portal` — portail captif & web

- **Fonction** : interface web Axum ; **une seule source de routage prod ET dev**.
- **Contexte** : reproduire la détection portail captif multi-OS (RFC 8908
  `/.well-known/captive-portal` + Apple `hotspot-detect`, Android `generate_204`,
  Windows `ncsi.txt`, MIUI/Vivo/Samsung…). Reprendre l'UX wizard QR/STARTER.
- **Capture totale** : tout chemin **inconnu** (fichier absent du webroot) est
  **redirigé 307 vers `/`** (`ServeDir::fallback` — pas `not_found_service`, qui
  forcerait un 404 via `SetStatus`), pas de 404 sec — un
  visiteur qui tape n'importe quoi retombe sur le portail. Les fichiers réellement
  présents (`/img`, `/lib`, `/privacy.html`…) et les routes explicites restent
  servis normalement ; `/install` et `/admin` sont gardés par le cycle de vie.
- **Administration** : `/admin` est un **tableau de bord** servi sans gate
  serveur (aucun secret dans le HTML) ; il affiche d'abord un **formulaire de
  login**. La connexion (`POST /api/admin/login`) ouvre une **session par
  cookie** (`HttpOnly`, `SameSite=Strict`, pas de `Secure` car HTTP local).
  `require_admin` accepte ce cookie **ou** l'auth Basic (outils/tests), toujours
  sous le tarpit anti-force-brute. Le **retour aux valeurs d'usine**
  (`POST /api/admin/reset` → `Store::factory_reset`) efface la config, le mot de
  passe admin, l'alerte, les bulletins, les tuiles et les sessions (identité du
  nœud conservée) : la borne repasse en provisioning.
- **Carte du lieu hors-ligne** : le client captif n'atteint **que le nœud** ;
  les tuiles OSM sont donc **servies par le nœud** (`/tiles`, `ServeDir` sur
  `SOS_TILES_DIR`). Elles sont téléchargées à l'install via le **`curl` système**
  (`POST /api/admin/map` autour du GPS, grille 5×5 zoom 16) — **jamais** un client
  HTTPS Rust : `ring`/asm C **casserait** le binaire statique aarch64-musl pur
  Rust (lié par `rust-lld`, sans toolchain C). L'accueil rend une mosaïque centrée
  + marqueur + attribution © OSM, repli SVG si hors-ligne. Toute carte est
  **centrée sur le nœud**, sans jamais pister les clients (cf. vie privée).

### `sos-security` — identités & validation

- **Informations** : identité Ed25519 par nœud, clé du hidden service Tor v3 ;
  `keyring`. Aucune identité partagée entre nœuds. Clé de **publication** (distincte)
  pour signer les manifestes de version (`release::{sign,verify}_detached`).
- **Fonction** : génération/garde des clés, signatures (alertes + manifestes de
  version), validation des entrées.
- **Contexte** : identités unifiées dans Redb (fini PEM épars / htpasswd / tor).
  Interop PEM v2.5 conservée. Toujours considérer XSS, CSRF, injections, path
  traversal, SSRF, buffer exhaustion, DoS.

### `sos-cli` — administration

- **Fonction** : binaire d'admin. `health` = vitaux du nœud en JSON
  (température/mémoire/charge/disque ; analyse pure testée, sources illisibles →
  `null`) ; recette `just vitals`. `watchdog` = caresse `/dev/watchdog`
  (`bcm2835_wdt`) tant qu'une **sonde applicative** (connexion TCP au démon) le
  juge sain, désarmement propre au `Drop` (octet `V`). `sign-update`/`verify-update`
  = manifeste de version signé (empreinte SHA-256 + signature Ed25519) ; recette
  `just verify-release` avant `just update`. `update` = **MAJ OTA « pull »
  signée** (flotte, image Alpine diskless) : lit `update.conf` (FAT), `curl` le
  manifeste + binaire, vérifie signature + empreinte, **refuse tout downgrade**,
  délègue l'install au **slot A/B** FAT + reboot (déclenchée par `crond` via
  `sos-update` ; sélection/rollback au boot par `sos-boot-select` +
  `sos-update-confirm` sur la santé `/api/status`). Binaire d'usine conservé dans
  l'apkovl (anti-bricage) ; publication opérateur `just release-keygen` +
  `just publish-release` (clé publique épinglée `/etc/sosguide/release.pub`).
- **Contexte** : la sonde santé reprend `just health` en natif ; l'unité systemd
  du watchdog est branchée au déploiement (non lancée en test pour ne pas armer
  le chien hors service).

### `apps/sos-guide` — binaire principal

- **Fonction** : assemble les crates, pilote le cycle de vie, supervise les
  tâches (remplace l'orchestration de ~7 processus shell de la v2.5).
- **QQOQCP** : *Comment* — un seul démon privilégié, commandes internes typées,
  **zéro `sudo` depuis le web** (réduction radicale de la surface d'escalade).

### `firmware/esp32-lora` — nœud satellite

- **Fonction** : nœud LoRa `no_std`, consommation minimale ; émet/relaie les
  alertes au format de trame v2.5 (interop `sos-core`).
- **Contexte** : workspace **séparé** (cible/toolchain distinctes ; exclu de l'hôte
  et de `just sync`). **Initialisé** : cible **ESP32-C3 (RISC-V**, toolchain Rust
  standard, pas d'`espup`), `esp-hal` 1.x, `build_frame()` (`heapless`) ; **compile**
  (cross-compile, sans matériel — `cargo build --release`). *À venir avec le
  matériel : Embassy (figer les versions `esp-hal-embassy`/`esp-hal`), pilote SX127x
  (SPI), boucle mesh, deep-sleep.*

---

## Cycle de vie

- **`STATE_PROVISIONING`** (premier démarrage) : WiFi public désactivé, seul le
  SSID `SOS-SETUP-XXXXXXXX` est diffusé. L'admin fixe son mot de passe et
  configure le nœud. Génération de l'identité Reticulum, de la clé Ed25519 et
  du service Tor ; clés stockées dans Redb.
- **`STATE_EMERGENCY`** (configuré) : le WiFi de configuration disparaît.
  Activation automatique : AP public, portail captif, DNS local, Reticulum, Tor,
  synchronisation mesh. Transition **à chaud, sans reboot** (UX héritée v2.5).
- **WiFi : AP TOUJOURS OUVERT** (décidé 2026-06-28, cf. « Modèle d'accès réseau »).
  Aucune clé, aucun WPA : un kiosque d'urgence public ne dresse **aucun obstacle**
  devant un citoyen. Le SSID est une **constante partagée** (`WIFI_SSID`, dans
  `sos-core`) réutilisée par `sos-network`. L'**affiche papier** du lieu porte un
  **QR de jonction WiFi du réseau ouvert** (`WIFI:S:SOS-GUIDE;T:nopass;;`, SVG
  généré côté nœud par `qrcodegen`) : affiché en fin d'`/install` et réimprimable
  en `/admin` (`GET /api/admin/wifi-qr`). *La machinerie de clé WPA (`wifiPassword`,
  génération, bascule WPA↔ouvert) a été **entièrement retirée** le 2026-07-02 —
  seule subsiste une purge défensive de `wifiPassword` contre d'éventuelles
  configs héritées v2.5.*

## Modèle d'accès réseau — la meilleure méthode (décidé 2026-06-28)

> Tranché après discussion : c'est **la** méthode retenue. Sujette à veto humain
> via [`ASK.md`](ASK.md), mais c'est la base de travail.

**Principe : `wlan0` = AP TOUJOURS actif · `eth0` = uplink OPTIONNEL.**

- **`wlan0` diffuse en permanence le WiFi SOS-GUIDE** (+ portail captif). La borne
  est donc **toujours joignable sans aucune infrastructure** : pour l'**installation**
  (l'admin se connecte à l'AP du nœud), pour les citoyens, en grid-down/bunker.
  ⇒ **« l'install doit être opérationnelle même sans Ethernet » est satisfait par
  construction** : on ne dépend jamais d'un réseau tiers pour atteindre la borne.
- **`eth0` (Ethernet) = uplink optionnel.** Branché → la borne obtient une **sortie**
  (téléchargement des **tuiles OSM** à l'install, **MAJ OTA**, **Tor**, bulletins
  officiels) **et** devient aussi joignable sur le LAN du lieu via **mDNS
  `sosguide.local`**. Débranché → la borne **fonctionne pleinement en local**
  (AP), sans les extras dépendant d'une sortie : **pas de tuiles OSM** (dégradation
  gracieuse → repli carte SVG), pas d'OTA jusqu'au prochain uplink. **Rien de
  vital ne dépend de l'uplink.**
- **Radio unique, zéro conflit** : l'uplink est **filaire** (`eth0`), donc `wlan0`
  reste dédié à l'AP (un Pi n'a qu'une radio WiFi : client *et* AP simultanés sont
  fragiles → on ne s'y fie pas). *Le mode « wifi-client » actuel du Pi de dev
  (`192.168.1.133`, `wifi.conf`) est un confort de **développement**, pas la prod.*
- **Pourquoi « AP toujours actif » plutôt que « AP seulement si pas d'Ethernet »** :
  c'est **strictement plus fiable**. La borne reste joignable directement quoi qu'il
  arrive (LAN saturé, box en panne, coupure) ; l'Ethernet n'**ajoute** que des
  extras, il n'**enlève** jamais l'accès. Pour un équipement de vie, *toujours
  joignable* l'emporte sur l'économie d'un SSID.
- **Nom** : en AP, le DNS du nœud résout **tout** vers la borne + portail captif
  (la page s'ouvre seule). En LAN (eth0), `sosguide.local` via mDNS. Un domaine
  public `sos.guide` est **exclu** (exigerait Internet, pointerait ailleurs).
- **AP TOUJOURS OUVERT (décidé 2026-06-28).** Pas de clé WiFi : un kiosque
  d'urgence public ne doit dresser **aucun obstacle** devant un citoyen. C'est
  **sûr** parce que **`eth0` n'est JAMAIS NATé/routé vers les clients WiFi** :
  l'uplink est l'egress **du nœud seul** (OSM/OTA/Tor), jamais une passerelle
  Internet pour l'AP → **aucun surf possible** (rien à voler), en plus de
  l'isolation `FORWARD DROP` + rate-limit + `/admin` authentifié. *Conséquence :
  le modèle hérité « protégé en veille, ouvert en alerte » (clé `wifiPassword`,
  bascule WPA↔ouvert) est **abandonné** ; le QR de l'affiche encode simplement un
  réseau **ouvert** (`WIFI:S:SOS-GUIDE;T:nopass;;`). La machinerie de clé a été
  **entièrement retirée du code le 2026-07-02** (plan d'AP, hostapd, portail,
  pages `/install` et `/admin`).*

*État : `sos-network` a tout le code (gaté `SOS_NET_MODE`). Reste à **câbler ce
modèle dans l'image Alpine** (paquet `hostapd` + `avahi` aarch64, init de
sélection de mode, bascule `live`) et à le **tester sur Pi réel** — sans risque de
lockout puisque l'appliance Alpine n'a pas de SSH-sur-wlan0 (≠ Pi de dev).*

## Plateformes & build

- **Développement (modèle DEV-SUR-PI)** : on **édite** sur le PC x86_64
  (Manjaro) — le dépôt git y reste la **source de vérité** — puis on
  **synchronise** les sources vers le Raspberry Pi (`rsync`, `just sync`) et on
  **compile/exécute nativement sur le Pi** (`aarch64-unknown-linux-gnu`, glibc,
  Rust 1.95 + gcc déjà installés). **Pour l'itération de dev**, `just build`/`just
  pi` compilent **nativement sur le Pi** (glibc, rapide, deps C possibles). **Mais
  l'image produit (Alpine) reste cross-compilée `aarch64-unknown-linux-musl`
  statique, pur-Rust sans toolchain C** (lié par `rust-lld`) — d'où les choix
  conservés (tuiles OSM via `curl` système, QR via `qrcodegen`) : **ne jamais
  introduire de dep C** qui casserait le binaire musl de l'image. Les réglages
  cross-compile [`sosguide/rust-toolchain.toml`](sosguide/rust-toolchain.toml) et
  [`sosguide/.cargo/config.toml`](sosguide/.cargo/config.toml) ne sont **pas**
  synchronisés vers le Pi (exclus par `just sync`).
- **Déploiement / test** : `just run` (sync + build debug + lance le nœud sur le
  Pi, `http://192.168.1.133:18080/`), `just install` (sync + build release +
  binaire `/usr/local/bin` + webroot `/var/www` + service systemd). SSH/rsync
  vers `admin@192.168.1.133` via le [`justfile`](audit-control/justfile). Aucun
  autre script. `just check` (fmt/clippy/tests) reste **local** (feedback rapide).
- **Production (cible produit) : image Alpine Linux *diskless*** ([`image-alpine/`](image-alpine)) —
  OS-en-RAM sur Pi 4, app **statique aarch64-musl** chargée par **apkovl**, image
  `.img` **reproductible** (assemblée rootless en conteneur `alpine:3.21` :
  `mtools`/`mke2fs`/`sfdisk`, `SOURCE_DATE_EPOCH`). Persistance **SOSDATA en
  lecture seule** (ext4 `p2`) : Redb de travail en tmpfs + **instantané durable**
  (`sos-commit-db`), fenêtres rw ponctuelles (`sos-rw`), garde-fou ro périodique
  (`crond`/`sos-ro-guard`). Services **OpenRC** ; logs en RAM (nLPD). **MAJ de
  flotte = OTA « pull » signé** (slot binaire **A/B** sur la FAT `p1` + rollback
  santé — `sos-cli update`, `sos-boot-select`, `sos-update-confirm`). Config WiFi
  et OTA éditables sur la FAT (`wifi.conf`, `update.conf`) sans reflasher.
- **Socle de dev : Raspberry Pi OS Lite** (Debian, aarch64). Service **systemd**
  durci (`ProtectSystem=strict` + `ReadWritePaths=/var/lib/sos-guide`,
  `PrivateTmp`, `RestrictAddressFamilies`, `LockPersonality`…) —
  [`configs/sos-guide.service`](sosguide/configs/sos-guide.service), installé par
  `just install` (+ drop-in **journald `Storage=volatile`**). C'est le Pi
  `192.168.1.133` du dev — **boucle d'itération uniquement**, pas un produit :
  l'image PRODUIT est l'Alpine diskless ci-dessus ([`image-alpine/`](image-alpine)).
  _(L'ancien outillage d'image RPi OS Lite `sosguide/image/` — durcissement +
  rootfs RO `overlayroot` — a été retiré, superSédé par l'image Alpine.)_
- **Profils** (dans [`sosguide/Cargo.toml`](sosguide/Cargo.toml)) : dev `opt-level=1`,
  `debug=false`, `panic="abort"`, `incremental=false` ; release `opt-level="z"`,
  `lto="fat"`, `codegen-units=1`, `panic="abort"`, `strip=true`.

## Sécurité

Valider **toutes** les entrées utilisateur ; considérer systématiquement XSS,
CSRF, injections, path traversal, SSRF, buffer exhaustion, DoS. Secrets : jamais
dans Git, jamais en dur, jamais dans les logs. Un seul démon privilégié, zéro
`sudo` depuis le web. Logs en tmpfs (volatils), aucune donnée personnelle.

## Hérité v2.5 — à préserver

Ce que **Rust ne remplace pas** et qu'il faut garder (le reste de l'ancien
système est réécrit nativement) :

- **`hostapd`** (nl80211) pour l'AP WiFi — démon externe piloté par `sos-network`.
- **`tor`** (hidden service v3) — démon externe ; surface `.onion` restreinte.
- **Immuabilité du rootfs** — sous Raspberry Pi OS Lite via `overlayroot`
  (rootfs lecture seule), à défaut du Buildroot d'origine.
- **Watchdog matériel `bcm2835_wdt`** (15 s) avec sonde applicative ; le
  redémarrage des tâches est assuré nativement par systemd (`Restart=always`).
- **Conformité nLPD (RS 235.1)/RGPD** : logs volatils en tmpfs, pas de baux DHCP
  persistés (`dhcp-leasefile=/dev/null`), aucune donnée personnelle stockée.
- **Liste exhaustive des endpoints de détection de portail captif** (RFC 8908 +
  Apple/Android/Windows/MIUI/Vivo/Samsung…) — à reprendre verbatim dans `sos-portal`.
- **Interop** : format de trame LoRa JSON compact et clés Ed25519 PEM de la v2.5.
- **i18n 29 langues** avec repli sur le français (romanche inclus, exigence PCi-CH).

## Registre d'erreurs ([`ERRORS.md`](audit-control/ERRORS.md))

Toutes les erreurs du projet sont centralisées dans `ERRORS.md`, qui **évolue
automatiquement** :

- **Capture (sans token)** : toute commande `just build|check|pi` qui échoue
  dépose une trace brute (30 lignes max) dans l'inbox d'`ERRORS.md`.
- **Curation (agent)** : [`error-curator`](audit-control/agents/error-curator.md)
  trie l'inbox, déduplique, qualifie la cause, classe Actives/Résolues et
  intègre les erreurs runtime du Pi (`just logs`). Lancement : `just errors`
  (ponctuel) ou `/loop 30m` avec l'agent (surveillance continue).

Conséquence : **router les builds par `just`** (et non `cargo` direct) pour que
les échecs soient capturés.

## Workflow

1. Édite sur le PC, puis `just check` (fmt + clippy `-D warnings` + tests, local)
   avant tout commit.
2. Après chaque correction : étape datée dans `ROADMAP.md`, ligne dans
   `CHANGELOG.md` ; laisse `ERRORS.md` se remplir et lance `just errors` pour le curer.
3. `just sync` (rsync vers le Pi), `just run` (build natif debug + lancement) pour
   valider sur le Pi, `just install` pour le service systemd.
4. Mises à jour & monitoring : `just install` (1ʳᵉ pose du service + durcissement
   systemd + journald volatile), `just update` (MAJ binaire **atomique avec
   rollback** sur santé `/api/status`), `just os-update` (MAJ `apt` hors-bande),
   `just logs`
   (journal), `just journal` (erreurs runtime journald → inbox `ERRORS.md`),
   `just vitals` (sonde `sos-cli health` en JSON) / `just health` (vitaux shell) —
   jalons M5/M6 de la `ROADMAP.md`.
