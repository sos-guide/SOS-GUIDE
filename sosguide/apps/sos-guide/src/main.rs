//! Point d'entrée du nœud SOS-GUIDE.
//!
//! Démarre le runtime, initialise la journalisation, charge (ou génère)
//! l'identité du nœud puis sert le portail d'urgence.
//!
//! Configuration par variables d'environnement :
//! - `SOS_LISTEN`  : adresse d'écoute (défaut `0.0.0.0:80`) ;
//! - `SOS_WEBROOT` : racine web v2.5 (défaut `/var/www/sos-guide`) ;
//! - `SOS_TILES_DIR` : cache des tuiles OSM (défaut `/var/lib/sos-guide/tiles`) ;
//! - `SOS_NODE_ID` : identifiant du nœud (défaut : nom d'hôte) ;
//! - `SOS_PORTAL_URL` : URL annoncée aux clients captifs
//!   (défaut `http://10.0.0.1/`, comme en v2.5) ;
//! - `SOS_PRIVATE_KEY_PEM` : chemin d'une clé privée Ed25519 PEM v2.5
//!   (`/etc/sos-guide/node_private_key.pem`) — sinon identité éphémère ;
//! - `SOS_TRUSTED_NODES` : registre des pairs de confiance au format v2.5
//!   (défaut `/etc/sos-guide/trusted_nodes.json`) — sinon confiance au seul nœud ;
//! - `SOS_NET_MODE` : réseau local (`off` défaut / `simulate` / `live`) ;
//! - `SOS_RADIO_MODE` : radio LoRa (`off` défaut / `simulate` / `live`) ;
//! - `SOS_RADIO_DEVICE` : périphérique série/SPI LoRa (mode `live`) ;
//! - `SOS_GW_MODE` : passerelle Tor (`off` défaut / `simulate` / `live`) ;
//! - `SOS_GW_BIND` : bind loopback du manifeste (défaut `127.0.0.1:9099`) ;
//! - `SOS_GW_HS_DIR` : répertoire du service caché Tor (mode `live`) ;
//! - `SOS_DB` : base Redb de **travail** (défaut `/var/lib/sos-guide/sos-guide.redb` ;
//!   sur tmpfs/RAM en appliance Alpine *diskless*) ;
//! - `SOS_DB_DURABLE` : instantané Redb **durable** sur SOSDATA (ro) ; absent ⇒
//!   la base de travail est elle-même durable (déploiement Debian) ;
//! - `SOS_COMMIT_CMD` : commande privilégiée d'instantané du Redb
//!   (remonte SOSDATA rw → copie atomique → remonte ro), reçoit `<working> <target>` ;
//! - `SOS_RW_CMD` : commande de **fenêtre rw** sur SOSDATA pour les écritures de
//!   tuiles, reçoit `open` puis `close` ; absent ⇒ no-op (support déjà rw).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use sos_core::{AlertInbox, Lifecycle, RuntimeSignal};
use sos_gateway::{GatewayConfig, GatewayMode};
use sos_network::{NetworkConfig, NetworkMode};
use sos_pay::{broadcast::DEFAULT_BROADCAST_API, PayConfig, PayMode, Relay};
use sos_portal::{NodeState, PortalConfig};
use sos_radio::{PayChannels, RadioConfig, RadioMode};
use sos_security::KeyRing;
use sos_storage::Store;
use tokio::sync::{mpsc, watch, Mutex, RwLock};

/// Capacité de la file des trames d'alerte vers le maillage LoRa. Bornée :
/// l'émission radio est lente, on préfère perdre une trame en surcharge (logguée)
/// plutôt que bloquer le portail (`try_send` non bloquant côté portail).
const RADIO_QUEUE: usize = 64;

/// Capacité de la file des fragments de paiement vers le mesh. Best-effort : les
/// alertes priment (sélection biaisée côté `sos-radio`).
const PAY_QUEUE: usize = 128;

fn main() -> ExitCode {
    if let Err(err) = run() {
        eprintln!("sos-guide: erreur fatale: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}

/// Identifiant de nœud : `SOS_NODE_ID`, sinon le nom d'hôte, sinon un défaut.
fn node_id() -> String {
    if let Ok(id) = std::env::var("SOS_NODE_ID") {
        return id;
    }
    std::fs::read_to_string("/etc/hostname")
        .map(|h| h.trim().to_owned())
        .ok()
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "sos-guide".to_owned())
}

/// Ouvre la base de persistance (best-effort). Un échec n'est pas fatal : le
/// nœud doit servir même si le disque n'est pas inscriptible (mode dégradé,
/// identité éphémère). Le dossier parent est créé si besoin.
///
/// `SOS_DB` est la base **de travail** (sur tmpfs/RAM dans l'appliance Alpine
/// *diskless*). Si `SOS_DB_DURABLE` est défini (instantané sur SOSDATA), la base
/// est ouverte en mode durable : restaurée au boot depuis l'instantané, puis
/// recopiée vers SOSDATA après chaque écriture — via `SOS_COMMIT_CMD` (commande
/// privilégiée remount-rw/copie/remount-ro) si SOSDATA est en lecture seule,
/// sinon par copie atomique en place. Sans `SOS_DB_DURABLE`, comportement
/// historique (la base de travail est elle-même durable).
fn open_store() -> Option<Store> {
    let path = PathBuf::from(env_or("SOS_DB", "/var/lib/sos-guide/sos-guide.redb"));
    if let Some(parent) = path.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            tracing::warn!(dir = %parent.display(), %err, "base non créable — identité éphémère");
            return None;
        }
    }
    let durable = std::env::var("SOS_DB_DURABLE")
        .ok()
        .filter(|s| !s.is_empty());
    let commit_cmd = std::env::var("SOS_COMMIT_CMD")
        .ok()
        .filter(|s| !s.is_empty());
    let opened = match &durable {
        Some(target) => Store::open_durable(&path, target, commit_cmd.clone()),
        None => Store::open(&path),
    };
    match opened {
        Ok(store) => {
            match (&durable, &commit_cmd) {
                (Some(target), Some(cmd)) => tracing::info!(
                    working = %path.display(), durable = %target, commit = %cmd,
                    "base de persistance ouverte (instantané durable, SOSDATA ro)"
                ),
                (Some(target), None) => tracing::info!(
                    working = %path.display(), durable = %target,
                    "base de persistance ouverte (instantané durable, copie en place)"
                ),
                _ => tracing::info!(path = %path.display(), "base de persistance ouverte"),
            }
            Some(store)
        }
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "base illisible — identité éphémère");
            None
        }
    }
}

/// Persiste l'identité ; un échec d'écriture est seulement journalisé.
fn persist_identity(store: &Store, ring: &KeyRing) {
    match ring.private_key_pem() {
        Ok(pem) => {
            if let Err(err) = store.save_identity(ring.node_id(), &pem) {
                tracing::warn!(%err, "persistance de l'identité impossible");
            }
        }
        Err(err) => tracing::warn!(%err, "export PEM de l'identité impossible"),
    }
}

/// Résout l'identité du nœud, par ordre de priorité :
/// 1. `SOS_PRIVATE_KEY_PEM` (clé v2.5 explicite) — chargée puis persistée ;
/// 2. identité déjà stockée dans Redb ;
/// 3. nouvelle identité générée, puis persistée.
///
/// Sans base inscriptible, l'identité reste éphémère (perdue au redémarrage).
fn load_keyring(
    node_id: &str,
    store: Option<&Store>,
) -> Result<KeyRing, Box<dyn std::error::Error>> {
    if let Ok(path) = std::env::var("SOS_PRIVATE_KEY_PEM") {
        let pem = std::fs::read_to_string(&path)?;
        let ring = KeyRing::from_private_key_pem(node_id, &pem)?;
        tracing::info!(path, "identité chargée depuis la clé PEM v2.5");
        if let Some(store) = store {
            persist_identity(store, &ring);
        }
        return Ok(ring);
    }
    if let Some(store) = store {
        if let Some(id) = store.load_identity()? {
            let ring = KeyRing::from_private_key_pem(&id.node_id, &id.private_key_pem)?;
            tracing::info!(node_id = id.node_id, "identité chargée depuis Redb");
            return Ok(ring);
        }
        let ring = KeyRing::generate(node_id);
        persist_identity(store, &ring);
        tracing::info!(node_id, "nouvelle identité générée et persistée dans Redb");
        return Ok(ring);
    }
    tracing::warn!("identité éphémère générée — base de persistance indisponible");
    Ok(KeyRing::generate(node_id))
}

/// Charge le registre des nœuds de confiance dans le trousseau.
///
/// Priorité : le **registre persisté dans Redb** (géré par l'admin via `/admin`)
/// fait foi ; à défaut, on **importe** le fichier `trusted_nodes.json` v2.5
/// (`SOS_TRUSTED_NODES`, défaut `/etc/sos-guide/trusted_nodes.json`) et on le
/// persiste pour qu'il devienne administrable. Sans aucun registre, le nœud ne
/// fait confiance qu'à **sa propre clé** : il rejette alors toute alerte mesh
/// d'un autre nœud (`Untrusted`) — défaut **sûr** mais isolant. Toute absence ou
/// erreur n'est pas fatale.
fn load_trusted_nodes_into(keyring: &mut KeyRing, store: Option<&Store>) {
    // 1. Registre persisté (source de vérité administrable).
    if let Some(store) = store {
        if let Ok(Some(json)) = store.load_trusted() {
            match keyring.load_trusted_nodes(&json) {
                Ok(count) => {
                    tracing::info!(count, "registre des nœuds de confiance chargé (Redb)");
                    return;
                }
                Err(err) => tracing::warn!(%err, "registre persisté illisible — repli fichier"),
            }
        }
    }
    // 2. Import du fichier v2.5, persisté pour devenir administrable.
    let path = env_or("SOS_TRUSTED_NODES", "/etc/sos-guide/trusted_nodes.json");
    match std::fs::read_to_string(&path) {
        Ok(json) => match keyring.load_trusted_nodes(&json) {
            Ok(count) => {
                tracing::info!(
                    path,
                    count,
                    "registre des nœuds de confiance importé (fichier)"
                );
                if let Some(store) = store {
                    if let Err(err) = store.save_trusted(&json) {
                        tracing::warn!(%err, "persistance du registre importé impossible");
                    }
                }
            }
            Err(err) => tracing::warn!(path, %err, "registre de confiance illisible — ignoré"),
        },
        Err(_) => {
            tracing::info!("aucun registre de confiance — confiance limitée au nœud lui-même")
        }
    }
}

/// SSID de configuration propre au nœud : `SOS-SETUP-XXXXXXXX`, suffixe dérivé
/// de façon déterministe de l'identité (FNV-1a), stable d'un démarrage à l'autre.
fn setup_ssid(node_id: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in node_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("SOS-SETUP-{:08X}", (hash & 0xFFFF_FFFF) as u32)
}

/// Construit la configuration réseau à partir de l'environnement et du store.
/// Le mode par défaut est `off` : rien n'est démarré tant que `SOS_NET_MODE`
/// n'est pas explicitement positionné (l'unique `wlan0` du Pi est la ligne SSH).
/// L'AP est toujours **ouvert** (cf. `sos-network::plan`) : aucune clé à charger.
fn network_config(node_id: &str, _store: Option<&Store>) -> NetworkConfig {
    NetworkConfig {
        mode: NetworkMode::from_env_value(&env_or("SOS_NET_MODE", "off")),
        setup_ssid: setup_ssid(node_id),
        ..NetworkConfig::default()
    }
}

/// Construit la configuration radio depuis l'environnement. Mode `off` par
/// défaut : aucune tâche radio tant que `SOS_RADIO_MODE` n'est pas positionné
/// (aucun matériel LoRa n'est branché sur le Pi de dev).
fn radio_config() -> RadioConfig {
    RadioConfig {
        mode: RadioMode::from_env_value(&env_or("SOS_RADIO_MODE", "off")),
        device: env_or("SOS_RADIO_DEVICE", "/dev/ttyUSB0"),
        ..RadioConfig::default()
    }
}

/// Construit le relais de paiement « Bitcoin tx over LoRa » selon `SOS_PAY_MODE`.
/// Défaut `off` ⇒ `None` : les endpoints `/api/pay` répondent « désactivé », aucun
/// impact sur le portail vital. La borne ne détient **ni clé ni fonds** (transport
/// de transactions signées uniquement). `live`/matériel LoRa restent différés.
fn build_pay_relay() -> Option<Arc<Mutex<Relay>>> {
    match PayMode::from_env_value(&env_or("SOS_PAY_MODE", "off")) {
        PayMode::Off => None,
        mode => {
            let config = PayConfig {
                mode,
                broadcast_api: env_or("SOS_PAY_BROADCAST_API", DEFAULT_BROADCAST_API),
            };
            tracing::info!(?mode, api = %config.broadcast_api, "paiement: relais Bitcoin/LoRa actif (transport seul, sans clé ni fonds)");
            Some(Arc::new(Mutex::new(Relay::new(config))))
        }
    }
}

/// Construit la configuration de la passerelle Tor depuis l'environnement. Mode
/// `off` par défaut : aucun service exposé tant que `SOS_GW_MODE` n'est pas
/// positionné. La surface `.onion` se limite au manifeste (jamais le portail).
fn gateway_config(node_id: &str) -> GatewayConfig {
    let bind = env_or("SOS_GW_BIND", "127.0.0.1:9099")
        .parse()
        .unwrap_or_else(|_| ([127, 0, 0, 1], 9099).into());
    GatewayConfig {
        mode: GatewayMode::from_env_value(&env_or("SOS_GW_MODE", "off")),
        manifest_bind: bind,
        hs_dir: env_or("SOS_GW_HS_DIR", "/var/lib/tor/sos-guide"),
        node_id: node_id.to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

/// Libellé d'un mode de sous-système (`off`/`simulate`/`live`) pour `/admin`.
fn mode_label(mode: impl std::fmt::Debug) -> String {
    format!("{mode:?}").to_lowercase()
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let listen: SocketAddr = env_or("SOS_LISTEN", "0.0.0.0:80").parse()?;
    let webroot = PathBuf::from(env_or("SOS_WEBROOT", "/var/www/sos-guide"));
    let portal_url = env_or("SOS_PORTAL_URL", "http://10.0.0.1/");
    let tiles_dir = PathBuf::from(env_or("SOS_TILES_DIR", "/var/lib/sos-guide/tiles"));
    let node_id = node_id();

    // Le webroot (fichiers statiques du portail) est optionnel : sur un nœud
    // fraîchement provisionné il n'existe pas encore. Le nœud démarre quand
    // même ; seul le service de fichiers sera vide tant que le webroot manque.
    if !webroot.is_dir() {
        tracing::warn!(
            webroot = %webroot.display(),
            "webroot absent — fichiers statiques indisponibles tant que non déployés"
        );
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;

    runtime.block_on(async {
        let store = open_store();
        let mut keyring = load_keyring(&node_id, store.as_ref())?;
        // Registre des pairs de confiance : sans lui, le maillage est inopérant
        // (les alertes des autres nœuds sont rejetées comme non fiables).
        load_trusted_nodes_into(&mut keyring, store.as_ref());

        // Phase du cycle de vie déduite de la config persistée : un nœud non
        // installé est en provisioning, sinon en urgence (cf. sos-core).
        let installed = store.as_ref().and_then(|s| s.config_installed().ok());
        let installed = installed.unwrap_or(false);
        let phase = Lifecycle::from_installed(installed);
        tracing::info!(node_id, %listen, phase = phase.wire_name(), "nœud SOS-GUIDE démarré");

        // Canal de transition à chaud : le portail émet l'état (installation,
        // alerte), le réseau s'y abonne pour basculer l'AP. État initial reflété
        // depuis la config et l'alerte persistées (cohérence après reboot).
        let alert_active = store
            .as_ref()
            .and_then(|s| s.load_active_alert().ok())
            .flatten()
            .is_some();
        let (alert_tx, alert_rx) = watch::channel(RuntimeSignal {
            installed,
            alert_active,
        });

        // Orchestrateur réseau, gaté par SOS_NET_MODE (off par défaut : no-op).
        let net_cfg = network_config(&node_id, store.as_ref());
        let net_mode = mode_label(net_cfg.mode);
        tokio::spawn(async move {
            if let Err(err) = sos_network::run(net_cfg, alert_rx).await {
                tracing::error!(%err, "réseau: orchestrateur arrêté");
            }
        });

        // Passerelle Tor, gatée par SOS_GW_MODE (off par défaut). Réutilise le
        // canal d'état (phase + alerte) pour bâtir le manifeste public.
        let gw_cfg = gateway_config(&node_id);
        let gw_mode = mode_label(gw_cfg.mode);
        let gw_rx = alert_tx.subscribe();
        tokio::spawn(async move {
            if let Err(err) = sos_gateway::run(gw_cfg, gw_rx).await {
                tracing::error!(%err, "passerelle: orchestrateur arrêté");
            }
        });

        // État partagé entre le portail et la radio : le trousseau (vérif des
        // signatures + rotation à chaud) et la boîte de réception (alertes mesh
        // affichées par le portail).
        let keyring = Arc::new(RwLock::new(keyring));
        let inbox = Arc::new(Mutex::new(AlertInbox::new()));

        // Relais de paiement « Bitcoin tx over LoRa » partagé (portail = dépôt/file ;
        // radio = émission des fragments + réassemblage des entrants). `None` si
        // `SOS_PAY_MODE=off`. Canal des fragments à diffuser (best-effort, alertes-first).
        let pay_relay = build_pay_relay();
        let pay_enabled = pay_relay.is_some();
        let (pay_tx, pay_rx) = mpsc::channel::<String>(PAY_QUEUE);
        let pay_channels = PayChannels {
            outgoing: pay_rx,
            relay: pay_relay.clone(),
        };

        // Canal des trames d'alerte à diffuser sur le maillage LoRa. Le portail
        // y pousse les alertes publiées localement ; la radio les émet.
        let (radio_tx, radio_rx) = mpsc::channel::<String>(RADIO_QUEUE);
        let radio_cfg = radio_config();
        let radio_mode = mode_label(radio_cfg.mode);
        let radio_keyring = Arc::clone(&keyring);
        let radio_inbox = Arc::clone(&inbox);
        tokio::spawn(async move {
            if let Err(err) =
                sos_radio::run(radio_cfg, radio_keyring, radio_inbox, radio_rx, pay_channels).await
            {
                tracing::error!(%err, "radio: orchestrateur arrêté");
            }
        });

        let config = PortalConfig {
            listen,
            webroot,
            portal_url,
            tiles_dir,
            subsystems: sos_portal::SubsystemModes {
                network: net_mode,
                radio: radio_mode,
                gateway: gw_mode,
            },
            // Fenêtre rw sur SOSDATA pour les écritures de tuiles (modèle Alpine
            // diskless, SOSDATA ro). Absent ⇒ no-op (déploiement Debian rw).
            rw_cmd: std::env::var("SOS_RW_CMD").ok().filter(|s| !s.is_empty()),
        };
        let node = NodeState {
            keyring,
            inbox,
            store,
            alert_tx: Some(alert_tx),
            radio_tx: Some(radio_tx),
            pay: pay_relay,
            // Le portail ne pousse des fragments que si le paiement est actif.
            pay_tx: pay_enabled.then_some(pay_tx),
        };
        sos_portal::serve(config, node).await?;
        Ok(())
    })
}
