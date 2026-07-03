# ASK.md — Dialogue d'évolution SOS-GUIDE

> **À quoi sert ce fichier.** Un lieu unique où **toi (humain)** et **moi
> (Claude)** posons nos questions sur l'évolution du projet, et où l'on **trace
> les décisions**. Objectif : ne plus tourner en rond, garder le cap *vital et
> d'intérêt public* ([`CLAUDE.md`](CLAUDE.md)).
>
> **Mode d'emploi.** Écris tes questions sous « *Questions — de toi à Claude* ».
> Je mets les miennes sous « *Questions — de Claude à toi* ». On répond **en
> dessous** de chaque question (préfixe `R:`), puis on déplace ce qui est tranché
> dans « *Décisions prises* ». Rien ne se perd.

---

## ✅ Décisions prises (récent en haut)

- **2026-07-03 — Deux modes produit + paiement mesh : Monero abandonné, «
  Bitcoin tx over LoRa » conservé (piste v2, hors cœur vital).**
  - **Modes** : *normal* = WiFi local + Internet + LoRa mesh ; *urgence* = WiFi
    local + LoRa mesh (sans Internet).
  - **Monero : abandonné.** Une tx Monero (~1,5–2,5 Ko) est le pire payload
    possible pour LoRa (~200 o utiles/message, duty cycle EU 868 = ~36 s/h) :
    ~10–15 messages fragmentés = plusieurs minutes d'airtime **par paiement**,
    saturation immédiate. Réservé jadis au mode normal, retiré du périmètre.
  - **Bitcoin tx over LoRa : conservé (idée v2).** Rôle de LoRa = **store-and-
    forward** d'une tx **signée** jusqu'à un **nœud-sortie encore connecté à
    Internet** qui la diffuse (prior art : *TxTenna*). Payload Bitcoin compact
    (~250–400 o = 1–2 messages LoRa), viable en **best-effort**.
  - **Règle non négociable** : LoRa reste le **canal d'alerte** (ligne de vie).
    Le relais de paiement est **best-effort, alertes-first, rate-limité** ; jamais
    prioritaire sur une alerte.
  - **Limites actées** : aucun réseau local ne **confirme** hors-ligne (confirmation
    = mineurs mondiaux) ; en **îlot total** (aucune sortie Internet dans la portée
    mesh) → pas de blockchain, repli **« ardoise » (crédit local différé)**. Risque
    de double-dépense hors-ligne assumé (petits montants / clients connus).
  - **Isolation** : tout ça vit dans une **édition commerçant** séparée (WiFi WPA,
    matériel mini-PC/NAS si nœud complet), **jamais sur le chemin du portail vital**
    qui doit rester opérationnel même si ce module est absent ou en panne.

- **2026-06-28 — Mode AP = hostapd + dnsmasq** (recette éprouvée), appli en
  `SOS_NET_MODE=off`. Pays WiFi **FR par défaut, éditable** (`ap.conf` sur la
  carte). **Image `.img` reconstruite, opérationnelle, vérifiée** (init
  `sos-netmode`, AP ouvert + portail captif + eth0/mDNS optionnel, zéro surf).
  **Reste = test sur Pi réel** (seul le matériel prouve « opérationnel »).
- **2026-06-28 — Traces réseau perso du dev : purgées** de l'image (instruction
  humaine). Mode client de test retiré.

- **2026-06-28 — Modèle d'accès réseau : `wlan0` AP TOUJOURS actif + `eth0`
  uplink optionnel.** *La meilleure méthode* (cf. CLAUDE.md « Modèle d'accès
  réseau »). Garantit que l'install et le secours marchent **sans aucune
  infrastructure** ; l'Ethernet n'ajoute que des extras (OSM, OTA, Tor), jamais
  du vital. **✅ Confirmé par l'humain.**
- **2026-06-28 — AP WiFi : TOUJOURS OUVERT** (plus de clé en veille). Kiosque
  d'urgence public = zéro obstacle pour un citoyen. Sûr car **`eth0` n'est jamais
  NATé vers les clients** (portail captif seul, **aucun surf Internet**) → rien à
  voler, isolation `FORWARD DROP`, rate-limit. *Conséquence : la machinerie clé
  WiFi/WPA-en-veille devient inutile ; le QR de l'affiche encode juste un réseau
  ouvert.*
- **2026-06-28 — Nom sur le LAN : `sosguide.local` (mDNS).** ✅
- **2026-06-28 — LoRa : prévoir les DEUX** matériels — **SX1276 (HAT SPI)** *et*
  **T-Beam (USB)** — via la couche `device` abstraite de `sos-radio`.
- **2026-06-28 — Distribution = l'image Alpine elle-même** (le `.img` est le
  produit ; `webpage/` est la vitrine/contact). Pas de page BOM séparée pour l'instant.
- **2026-06-28 — En *standby*** : relecture humaine des 28 traductions ;
  certification Croix-Rouge Suisse (à 0 aujourd'hui — pas un moteur de priorité).
- **2026-06-28 — Chat embarqué : écarté.** Même le chat local WiFi ajoute surface
  d'attaque + modération + responsabilité, pour un gain incertain. Le Pi doit
  faire l'évident. *(Voir « Idées rangées ».)*
- **2026-06-27 — MAJ de flotte : OTA « pull » signé + reboot + cron auto.** Slot
  A/B sur la FAT, anti-downgrade, rollback santé ; binaire d'usine préservé.
- **2026-06-27 — Associations & numéros d'aide : configurables**, suivent le
  **pays** du déploiement (plus la langue). Éditeur à l'install et dans `/admin`.
- **2026-06-26 — OS produit : Alpine Linux diskless** (OS-en-RAM), SOSDATA ro.

## ❓ Questions — de Claude à toi *(en attente de ta réponse)*

1. **Modèle réseau AP-always + eth0** : tu valides tel quel, ou tu veux ajuster ?
   *R: AP-always + eth0
2. **AP en veille : protégé par clé (affichée sur l'affiche du lieu) OU toujours
   ouvert ?** Pour un kiosque d'urgence public, « toujours ouvert » est le plus
   accessible (aucun obstacle pour un citoyen) ; « protégé en veille, ouvert en
   alerte » limite l'usage occasionnel. Le code sait faire les deux.
   *R:toujours ouvert
3. **Nom sur le LAN** : `sosguide.local` (mDNS) te convient ?
   *R:oui
4. **Matériel LoRa** : on vise quoi en premier — **SX1276 (HAT SPI)** ou
   **T-Beam (USB)** — et tu as le matériel quand ?
   *R:pense au 2 methodes
5. **Traductions** : les 28 langues non-FR sont des **traductions auto à faire
   relire par un humain** (règle : jamais d'auto-traduction des consignes
   vitales). Qui relit, et est-ce bloquant pour une 1ʳᵉ diffusion citoyenne ?
   *R:standby pour l'instant
6. **Certification Croix-Rouge Suisse** : où en es-tu (interlocuteur, exigences
   connues) ? Ça oriente les priorités sécurité/conformité.
   *R: a 0 pour l'instant
7. **Distribution / kit matériel** : on prépare un BOM + une page « obtenir une
   borne » maintenant, ou plus tard ?
   *R:AlpineLinux
8. **Serveur de publication OTA** : tu veux que je prépare un `just serve-release`
   (sert `publish/` en local pour tester le cycle de MAJ) ?
   *R:* *(laissé vide — je propose : pas bloquant, je l'ajouterai au moment du test OTA réel. On peut s'en passer pour l'instant.)*

## ❓ Questions — de toi à Claude *(écris ici, je réponds en dessous)*

- *As-tu bien pris conscience de `audit-control`, `image-alpine`, `sosguide`, `webpage` ?*
  *R: Oui — et j'ai relu pour de vrai (`webpage` je ne l'avais jamais ouvert). Voici ce que chacun EST :*
  - **`sosguide/`** = **le produit logiciel** (workspace Rust hexagonal) : 8 crates
    (`core/radio/gateway/storage/network/portal/security/cli`) + `apps/sos-guide`
    + `firmware/esp32-lora` (workspace séparé) + **`web/`** = le **portail de
    l'appareil** (index/install/admin + `lib/` + `data/` 29 langues). C'est ce qui
    tourne sur le Pi.
  - **`image-alpine/`** = **le produit distribué** : fabrique l'`.img` Alpine
    *diskless* (`assemble.sh` reproductible), `overlay/` (apkovl : services OpenRC,
    helpers `sos-commit-db`/`sos-rw`/`sos-ro-guard`/OTA, `etc/sosguide`, crontab),
    `boot/` (`wifi.conf`, `update.conf`), `boot-extra/aarch64/` (apks WiFi),
    `data-skel/` (graine SOSDATA), `pinned/` (version Alpine), `out/` (l'image).
  - **`audit-control/`** = **gouvernance/dev, hors image** : `justfile` (tout
    l'outillage), `audit-gen.py` (génère `audit.html` depuis ROADMAP+ERRORS, cases
    à cocher → prompt de session suivante), `ERRORS.md` (registre vivant, capture
    auto + curation), `agents/` (`rust-architect`, `error-curator`),
    `settings.local.json`.
  - **`webpage/`** = **le site public / vitrine** sosguide.fr (≠ portail appareil) :
    hero « Panne générale ? Pas de panique ! », présentation du boîtier, **formulaire
    de commande** (`submit-order.php` → org/type/quantité/lieu/numéros locaux →
    e-mail `contact@sos-guide.fr`), contact, démo, `intro.mp4`, blog, pages légales
    (mentions/CGU/RGPD), i18n 29 langues. C'est la **face distribution/commerciale**.

  *La cohérence d'ensemble : `webpage` (commande + pré-config) → `image-alpine`
  (l'`.img` pré-configuré) → `sosguide` (le portail qui tourne) → `audit-control`
  (build/gouvernance). Le formulaire de commande pré-collecte même le lieu + les
  numéros d'urgence = la config d'install. Joli pipeline.*

  *3 observations honnêtes : (1) `webpage` est du **PHP legacy** hébergé à part —
  à ne PAS confondre avec le portail Rust ; (2) `webpage/data/*.json` et
  `sosguide/web/data/*.json` sont **deux jeux i18n distincts** (site vs appareil) ;
  (3) `audit-control/settings.local.json` contient du **résidu obsolète** (typo
  `sosguie.fr`, anciens chemins `sg-box`/`sg-claude`) — ménage possible.*
-

## 🗄️ Idées rangées (volontairement écartées — réversible)

- **Chat local WiFi + messagerie inter-nœuds autorisée (façon Session/Reticulum).**
  Vision cohérente (relais LoRa + Tor déjà là, identités Ed25519, registre de
  confiance admin), mais **écartée pour l'instant** : surface d'attaque,
  modération, responsabilité juridique, et bande passante LoRa minuscule. À
  **reconsidérer après** mise en service réelle si un besoin terrain le prouve —
  et alors **par paliers** (chat local éphémère RAM d'abord, contacts autorisés
  ensuite). Le « ping de détresse » de la carte couvre déjà l'alerte minimale.
