# 🛡️ SOS-GUIDE

> Plateforme souveraine de communication d'urgence, conçue pour fonctionner en
> mode dégradé ou **totalement hors-Internet**.

En cas de catastrophe naturelle, de coupure réseau/électrique ou de
cyberattaque, un nœud SOS-GUIDE diffuse des informations critiques via un
réseau local autonome (WiFi + portail captif), un maillage radio longue portée
(LoRa/Reticulum) et des liens chiffrés (Tor) — sur du matériel peu puissant et
malgré les coupures.

**Statut : v0.1.0 — squelette d'architecture.** Réécriture Rust d'un système
legacy v2.5 (PHP/Bash/Python).

## Architecture

Rust stable, architecture hexagonale (Ports & Adapters), DDD léger. Détail et
spécification complète : [`CLAUDE.md`](CLAUDE.md).

```
V1/
├── CLAUDE.md            # spécification & blueprint (à la racine, chargé chaque session)
├── docs/                # toute la documentation de pilotage
│   ├── README.md        # ce fichier
│   ├── ROADMAP.md       # plan ordonné de réalisation + journal
│   └── CHANGELOG.md     # journal des versions
├── audit-control/       # outillage Claude (hors de l'img produit)
│   ├── justfile         # build, déploiement, audit (le seul « script »)
│   ├── audit-gen.py     # générateur de la page d'audit locale (just audit)
│   ├── ERRORS.md        # registre central des erreurs (auto-alimenté + curé)
│   └── agents/          # error-curator, rust-architect
└── sosguide/            # LE PRODUIT (devient l'img propre)
    ├── Cargo.toml       # workspace Rust
    ├── crates/          # core · radio · gateway · storage · network · portal · security · cli
    ├── apps/sos-guide   # binaire principal
    ├── web/             # webroot servi par le portail (page d'accueil, 29 langues, thème)
    └── firmware/        # nœud ESP32 LoRa (workspace séparé)
```

## Prérequis (PC de développement)

- Rust stable (`rustup`)
- [`just`](https://github.com/casey/just) pour l'orchestration
- La cible de cross-compilation : `rustup target add aarch64-unknown-linux-musl`
  (ou `just target`)

## Build & test

Les recettes vivent dans [`audit-control/justfile`](audit-control/justfile) :

```sh
cd audit-control

just            # liste les recettes
just build      # compilation hôte (x86_64)
just check      # cargo fmt + clippy -D warnings + tests
```

## Déploiement sur le Raspberry Pi

Le binaire est compilé sur le PC (x86_64) puis **copié sur le Pi par SSH/SCP**
(ethernet, sans WiFi) :

```sh
cd audit-control

just pi          # build release cross-compilé (aarch64-musl, statique)
just deploy      # build + scp du binaire vers pi@raspberrypi.local:/tmp/sos-guide
just run         # déploie puis exécute le binaire sur le Pi
```

Cible SSH par défaut : `admin@192.168.1.133` (modifiable en tête du `justfile`).

## Mises à jour & monitoring

```sh
cd audit-control

just install     # met à jour le binaire + service systemd sur le Pi
just logs        # journal systemd du nœud (journald)
just health      # vitaux du Pi : température, RAM, charge, disque, état du service
```

Voir les phases 5 (mises à jour) et 6 (image propre) dans [`ROADMAP.md`](ROADMAP.md).

## Licence

AGPL-3.0-only.
