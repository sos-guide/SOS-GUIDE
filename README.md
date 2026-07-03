<div align="center">

# ⛑️ SOS-GUIDE

**L'information qui sauve, même sans réseau.**
*The information that saves lives, even off-grid.*

Une borne d'urgence **autonome, souveraine et hors-ligne** : un Raspberry Pi qui
**devient le réseau** et diffuse vos consignes vitales quand tout le reste a lâché.

![License](https://img.shields.io/badge/licence-AGPL--3.0-blue)
![Rust](https://img.shields.io/badge/Rust-stable-orange)
![Cible](https://img.shields.io/badge/cible-Raspberry%20Pi%204%20·%20Alpine%20diskless-informational)
![Statut](https://img.shields.io/badge/borne-validée%20sur%20Pi%20réel-success)
![Données](https://img.shields.io/badge/données%20personnelles-zéro-success)

</div>

---

## Pourquoi

Lors d'un **blackout**, d'une catastrophe ou d'une cyberattaque, les antennes
relais tombent en quelques heures. Plus de mobile, plus de WiFi, **plus
d'information** — au moment précis où elle est vitale.

**SOS-GUIDE est une bouée d'information locale.** Un boîtier qui crée son propre
réseau WiFi ouvert et affiche, dans le navigateur de n'importe quel téléphone,
les consignes de sécurité, les numéros d'urgence et une carte hors-ligne — **sans
Internet, sans cloud, sans collecte de données**.

> **⚠️ Équipement de survie.** Une borne qui ment, plante ou affiche un mauvais
> numéro peut coûter une vie. Ici, la **fiabilité prime sur tout** : contenu vital
> exact (jamais inventé ni auto-traduit), hors-ligne par construction, dégradation
> toujours gracieuse.

---

## ✨ Points clés

- 📶 **La borne *est* le réseau** — AP WiFi **toujours ouvert** + portail captif : aucun obstacle, la page s'ouvre seule.
- 🔌 **100 % hors-ligne** — aucun Internet requis pour secourir ; l'uplink Ethernet n'ajoute que des extras (tuiles OSM, MAJ, Tor), jamais du vital.
- 🦀 **Cœur en Rust** — un seul démon, binaire **statique aarch64-musl**, `unwrap`/`panic`/`unsafe` interdits, robuste aux coupures.
- 🧠 **Alpine Linux *diskless*** — OS en RAM, données en lecture seule : insensible aux coupures de courant, aucune usure de la carte SD.
- 🔒 **Zéro donnée personnelle** — conforme nLPD / RGPD **par conception**, logs volatils en mémoire.
- 🌍 **29 langues** avec repli français (romanche inclus).
- 🆘 **Alertes** — l'admin lève une alerte → page SOS plein écran (cause + consignes) ; propagation mesh entre bornes.
- 📡 **Maillage LoRa** — relais des alertes longue portée entre nœuds *(logiciel prêt, matériel requis)*.
- ₿ **Paiement résilient** — transport de transactions **Bitcoin signées** sur le mesh en mode urgence *(voir plus bas — désactivé par défaut)*.

---

## 🏗️ Architecture

Workspace Rust hexagonal (Ports & Adapters), dans [`sosguide/`](sosguide). Chaque
sous-système « risqué » est **gaté** (`off` par défaut) : le vital tourne, le reste
s'active à la demande.

| Crate | Couche | Rôle | Statut |
|---|---|---|---|
| [`core`](sosguide/crates/core) | Domaine | Alertes, cycle de vie, règles, manifeste de version | **mûr** |
| [`storage`](sosguide/crates/storage) | Infra | Redb durable + instantané SOSDATA lecture seule | **mûr** |
| [`security`](sosguide/crates/security) | Infra | Ed25519, Tor v3, signatures, validation des entrées | **mûr** |
| [`portal`](sosguide/crates/portal) | Interface | Portail captif Axum, `/install` + `/admin`, i18n | **mûr** |
| [`network`](sosguide/crates/network) | Infra | AP WiFi, DNS/DHCP locaux, isolation netfilter | code complet · gaté `SOS_NET_MODE` |
| [`radio`](sosguide/crates/radio) | Infra | LoRa : relais mesh des alertes **et** fragments de paiement | code complet · gaté `SOS_RADIO_MODE` |
| [`gateway`](sosguide/crates/gateway) | Infra | Tor v3 : manifeste `.onion` restreint | code complet · gaté `SOS_GW_MODE` |
| [`pay`](sosguide/crates/pay) | Domaine | Relais « Bitcoin tx over LoRa » (transport seul) | code complet · gaté `SOS_PAY_MODE` |
| [`cli`](sosguide/crates/cli) | Interface | `sos-cli` : santé, watchdog, sign/verify, MAJ OTA | **mûr** |
| [`apps/sos-guide`](sosguide/apps/sos-guide) | Application | Binaire principal : assemble et supervise | **mûr** |
| [`firmware/esp32-lora`](sosguide/firmware/esp32-lora) | Embarqué | Nœud satellite `no_std` (ESP32-C3) | initialisé |

Spécification d'ingénierie complète : [`CLAUDE.md`](CLAUDE.md) · journal de décisions : [`ASK.md`](ASK.md).

---

## 📡 Modèle d'accès réseau

**`wlan0` = AP toujours actif (ouvert)** · **`eth0` = uplink optionnel.**

La borne est **toujours joignable sans aucune infrastructure**. Branchez un câble
Ethernet et elle gagne une *sortie* (tuiles OSM, MAJ OTA, Tor) — mais **`eth0`
n'est jamais routé vers les clients WiFi** : aucun surf possible, isolation
`FORWARD DROP` + rate-limit. Débranchée, elle fonctionne pleinement en local.

## 🔀 Deux modes

| | **Normal** | **Urgence** |
|---|---|---|
| Connectivité | WiFi local **+ Internet** + LoRa | WiFi local + LoRa *(sans Internet)* |
| Info vitale | ✅ | ✅ |
| Cartes / MAJ / Tor | ✅ | dégradé (repli hors-ligne) |
| Paiement Bitcoin | confirmé via Internet | transporté sur le mesh vers un nœud-sortie |

---

## ₿ Paiement résilient (Bitcoin over LoRa)

En mode urgence, un commerçant peut continuer à encaisser : le client signe sa
transaction sur **son** téléphone, la borne la **transporte** (elle ne détient
**ni clé ni fonds**), la fragmente sur le mesh LoRa **en best-effort — les alertes
priment toujours** — jusqu'à un **nœud-sortie** encore connecté qui la diffuse.

> **Honnêteté d'ingénieur.** Aucun réseau local ne peut *confirmer* une transaction
> (c'est le rôle des mineurs mondiaux). En **îlot total** (aucune sortie Internet),
> la transaction reste en attente. Module **isolé, désactivé par défaut**
> (`SOS_PAY_MODE=off`), sans aucun impact sur le portail vital.

---

## 🧱 Build & image

Le produit final est une **image `.img` Alpine Linux *diskless* reproductible**.

```bash
# 1) Compiler les binaires statiques aarch64-musl (portail + CLI)
cd sosguide
cargo build --release --target aarch64-unknown-linux-musl -p sos-guide -p sos-cli

# 2) Assembler l'image (reproductible, rootless, en conteneur alpine:3.21)
cd ../image-alpine
./assemble.sh          # produit out/sosguide-<ver>.img (+ .sha256)

# 3) Flasher
xz -dc out/sosguide-*.img.xz | sudo dd of=/dev/sdX bs=4M status=progress conv=fsync
```

Au premier démarrage, la borne s'ouvre en mode **provisioning** : rejoignez le
WiFi ouvert **`SOS-GUIDE`**, la page `/install` apparaît.

**Qualité** : `cargo fmt` + `cargo clippy -D warnings` propres, tests inclus.
```bash
cargo test --workspace     # depuis sosguide/
```

---

## ✅ Statut

| Domaine | État |
|---|---|
| Portail d'urgence (info, alertes, carte, i18n) | ✅ **validé sur Raspberry Pi réel** |
| Image Alpine diskless reproductible | ✅ construite & flashée |
| AP WiFi ouvert + portail captif | ✅ (image Alpine) |
| Réseau / Radio / Tor / Paiement | 🟡 **code complet, testé, gaté `off`** |
| Maillage LoRa entre bornes | 🟡 logiciel prêt — **pilote `live` + matériel LoRa requis** |
| Relecture humaine des 28 traductions | ⏳ en cours |

> Les modules gatés sont **codés et testés** (en simulation) ; leur mode `live`
> attend le **matériel** (module LoRa SX1276/Meshtastic, démon Tor) pour être
> écrit et validé.

---

## 📚 Documentation

- [`CLAUDE.md`](CLAUDE.md) — spécification & blueprint d'ingénierie (3 niveaux de zoom).
- [`ASK.md`](ASK.md) — dialogue d'évolution & journal des décisions.
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — jalons + journal daté.
- [`docs/CHANGELOG.md`](docs/CHANGELOG.md) — versions (SemVer).

---

## ⚖️ Licence & crédits

Distribué sous **[AGPL-3.0-only](LICENSE)** — logiciel libre, souverain, d'intérêt public.

Créé par **Ludovic MARTIN**. Version historique (web/PHP) archivée :
[`sos-guide/SOS-GUIDE-v2.5`](https://github.com/sos-guide/SOS-GUIDE-v2.5).

<div align="center">
<sub>Conçu pour sauver des gens — pas pour capter des données.</sub>
</div>
