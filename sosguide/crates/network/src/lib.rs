//! Réseau local du nœud : point d'accès WiFi, DNS de portail captif, DHCP, et
//! isolation netfilter.
//!
//! # Modes d'exécution ([`NetworkMode`], via `SOS_NET_MODE`)
//!
//! - **`off`** (défaut) : l'orchestrateur ne démarre rien. Aucun socket, aucun
//!   processus, aucune mutation système. C'est le défaut de production tant que
//!   l'AP n'a pas d'interface dédiée sûre (l'unique `wlan0` du Pi est la ligne
//!   SSH : l'activer = lockout).
//! - **`simulate`** : lance DNS + DHCP sur des binds configurables (loopback,
//!   ports hauts) et journalise les transitions de plan d'AP, **sans aucune
//!   mutation système** (pas de `hostapd`, pas d'`iptables`, pas de config
//!   d'interface). Sert au test hors-ligne.
//! - **`live`** : `simulate` + configuration de l'interface, règles `iptables`,
//!   génération de la conf `hostapd` et (re)démarrage du démon. **Présent mais
//!   non exécuté sur ce Pi** tant qu'une interface AP sûre n'existe pas.
//!
//! La logique de décision (quel SSID, ouvert vs protégé) est dans [`plan`], pure
//! et testée ; les générateurs ([`hostapd`], [`firewall`], [`iface`]) et les
//! codecs ([`dns`], [`dhcp`]) sont eux aussi purs et testés. Seule l'exécution
//! des effets de bord (sockets, `tokio::process`) vit ici.

pub mod dhcp;
pub mod dns;
pub mod firewall;
pub mod hostapd;
pub mod iface;
pub mod plan;

use std::net::{Ipv4Addr, SocketAddr};

use sos_core::{RuntimeSignal, WIFI_SSID};
use tokio::sync::watch;

use crate::plan::{plan_for, ApPlan};

/// Canal WiFi 2,4 GHz par défaut de l'AP.
const DEFAULT_CHANNEL: u8 = 6;
/// Code pays par défaut (Suisse — déploiement de référence PCi-CH).
const DEFAULT_COUNTRY: &str = "CH";
/// Durée de bail DHCP (s) — courte, aucun bail n'est persisté.
const LEASE_SECS: u32 = 600;

/// Mode d'exécution du réseau, dérivé de `SOS_NET_MODE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NetworkMode {
    /// Ne rien démarrer (défaut sûr).
    #[default]
    Off,
    /// DNS + DHCP sur binds de test, sans mutation système.
    Simulate,
    /// Mode complet (interface + netfilter + hostapd). Non exécuté ici.
    Live,
}

impl NetworkMode {
    /// Interprète une valeur d'environnement. Toute valeur inconnue (ou absente)
    /// retombe sur [`NetworkMode::Off`] — le défaut sûr.
    #[must_use]
    pub fn from_env_value(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "simulate" | "sim" => Self::Simulate,
            "live" => Self::Live,
            _ => Self::Off,
        }
    }
}

/// Configuration du sous-système réseau.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Mode d'exécution.
    pub mode: NetworkMode,
    /// Interface radio de l'AP (ex. `wlan0`).
    pub iface: String,
    /// IP de la passerelle/nœud (serveur DNS, DHCP, routeur).
    pub gateway_ip: Ipv4Addr,
    /// Masque du réseau de l'AP.
    pub mask: Ipv4Addr,
    /// Première IP attribuable du pool DHCP.
    pub pool_start: Ipv4Addr,
    /// Dernière IP attribuable du pool DHCP.
    pub pool_end: Ipv4Addr,
    /// SSID de configuration (`SOS-SETUP-XXXX`).
    pub setup_ssid: String,
    /// SSID public d'urgence (constante partagée [`WIFI_SSID`]).
    pub public_ssid: String,
    /// Adresse de bind du DNS en mode `simulate` (loopback, port haut).
    pub dns_sim_bind: SocketAddr,
    /// Adresse de bind du DHCP en mode `simulate` (loopback, port haut).
    pub dhcp_sim_bind: SocketAddr,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            mode: NetworkMode::Off,
            iface: "wlan0".to_owned(),
            gateway_ip: Ipv4Addr::new(10, 0, 0, 1),
            mask: Ipv4Addr::new(255, 255, 255, 0),
            pool_start: Ipv4Addr::new(10, 0, 0, 10),
            pool_end: Ipv4Addr::new(10, 0, 0, 250),
            setup_ssid: "SOS-SETUP-00000000".to_owned(),
            public_ssid: WIFI_SSID.to_owned(),
            dns_sim_bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 15353)),
            dhcp_sim_bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 16767)),
        }
    }
}

/// Erreur du sous-système réseau.
#[derive(Debug, thiserror::Error)]
pub enum NetworkError {
    /// Échec d'une opération de socket (bind/IO).
    #[error("erreur d'entrée/sortie réseau : {0}")]
    Io(#[from] std::io::Error),
}

/// Calcule le plan d'AP courant à partir de la configuration et d'un signal.
fn current_plan(cfg: &NetworkConfig, signal: RuntimeSignal) -> ApPlan {
    plan_for(signal, &cfg.setup_ssid, &cfg.public_ssid)
}

/// Lance l'orchestrateur réseau. Ne retourne qu'en cas d'arrêt (mode `off`) ou
/// d'erreur fatale de bind ; sinon supervise les tâches DNS/DHCP et applique les
/// transitions de plan tant que le canal d'alerte est ouvert.
///
/// `alerts` reçoit les changements d'état runtime (installation, alerte) émis par
/// le portail ; chaque changement recalcule le plan d'AP.
pub async fn run(
    cfg: NetworkConfig,
    mut alerts: watch::Receiver<RuntimeSignal>,
) -> Result<(), NetworkError> {
    if cfg.mode == NetworkMode::Off {
        tracing::info!("réseau: mode off — aucune tâche démarrée");
        return Ok(());
    }

    // Plan initial.
    let mut plan = current_plan(&cfg, *alerts.borrow());
    tracing::info!(mode = ?cfg.mode, ssid = %plan.ssid, "réseau: démarrage (AP ouvert)");

    if cfg.mode == NetworkMode::Live {
        apply_live(&cfg, &plan).await;
    }

    // DNS : répond toute requête A par l'IP du nœud (portail captif).
    let dns_bind = match cfg.mode {
        NetworkMode::Live => SocketAddr::from((cfg.gateway_ip, 53)),
        _ => cfg.dns_sim_bind,
    };
    match dns::bind(dns_bind).await {
        Ok(socket) => {
            let node_ip = cfg.gateway_ip;
            tokio::spawn(async move { dns::serve(socket, node_ip).await });
            tracing::info!(%dns_bind, "réseau: DNS en écoute");
        }
        Err(err) => tracing::warn!(%err, %dns_bind, "réseau: DNS non démarré"),
    }

    // DHCP : attribue 10.0.0.10–250, sans bail persisté.
    let dhcp_bind = match cfg.mode {
        NetworkMode::Live => SocketAddr::from((cfg.gateway_ip, dhcp::SERVER_PORT)),
        _ => cfg.dhcp_sim_bind,
    };
    match dhcp::bind(dhcp_bind, cfg.mode == NetworkMode::Live).await {
        Ok(socket) => {
            let dhcp_cfg = dhcp::DhcpConfig {
                server_ip: cfg.gateway_ip,
                mask: cfg.mask,
                lease_secs: LEASE_SECS,
            };
            let pool = dhcp::LeasePool::new(cfg.pool_start, cfg.pool_end);
            tokio::spawn(async move { dhcp::serve(socket, dhcp_cfg, pool).await });
            tracing::info!(%dhcp_bind, "réseau: DHCP en écoute");
        }
        Err(err) => tracing::warn!(%err, %dhcp_bind, "réseau: DHCP non démarré"),
    }

    // Boucle de transition à chaud : à chaque signal, recalcule le plan ; si le
    // SSID ou la sécurité changent, régénère hostapd (live) ou journalise.
    loop {
        if alerts.changed().await.is_err() {
            tracing::info!("réseau: canal d'alerte fermé — fin de l'orchestrateur");
            return Ok(());
        }
        let next = current_plan(&cfg, *alerts.borrow());
        if next == plan {
            continue;
        }
        tracing::info!(ssid = %next.ssid, "réseau: transition de plan d'AP");
        if cfg.mode == NetworkMode::Live {
            apply_live(&cfg, &next).await;
        }
        plan = next;
    }
}

/// Applique l'état système en mode `live` : interface, netfilter, hostapd.
/// **Non atteint hors `live`.** Toute commande qui échoue est journalisée sans
/// interrompre les autres (best-effort, le réseau reste l'objectif principal).
async fn apply_live(cfg: &NetworkConfig, plan: &ApPlan) {
    let cidr = format!("{}/24", cfg.gateway_ip);
    for cmd in iface::iface_commands(&cfg.iface, &cidr) {
        run_command(&cmd.program, &cmd.args).await;
    }
    let params = firewall::FwParams {
        iface: cfg.iface.clone(),
    };
    for args in firewall::iptables_rules(&params) {
        run_command("iptables", &args).await;
    }
    let conf = hostapd::hostapd_conf(plan, &cfg.iface, DEFAULT_CHANNEL, DEFAULT_COUNTRY);
    if let Err(err) = tokio::fs::write("/etc/hostapd/hostapd.conf", conf).await {
        tracing::warn!(%err, "réseau: écriture hostapd.conf impossible");
    }
    // Recharge hostapd pour appliquer le nouveau plan (ouvert↔protégé à chaud).
    run_command("systemctl", &["restart".to_owned(), "hostapd".to_owned()]).await;
}

/// Exécute une commande système (mode `live` uniquement). Best-effort.
async fn run_command(program: &str, args: &[String]) {
    match tokio::process::Command::new(program)
        .args(args)
        .status()
        .await
    {
        Ok(status) if status.success() => {}
        Ok(status) => tracing::warn!(%program, ?status, "réseau: commande en échec"),
        Err(err) => tracing::warn!(%err, %program, "réseau: commande non lançable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn mode_parses_known_values_and_defaults_off() {
        assert_eq!(
            NetworkMode::from_env_value("simulate"),
            NetworkMode::Simulate
        );
        assert_eq!(NetworkMode::from_env_value(" LIVE "), NetworkMode::Live);
        assert_eq!(NetworkMode::from_env_value("off"), NetworkMode::Off);
        assert_eq!(
            NetworkMode::from_env_value("nimportequoi"),
            NetworkMode::Off
        );
        assert_eq!(NetworkMode::from_env_value(""), NetworkMode::Off);
    }

    #[test]
    fn default_config_is_off_with_shared_ssid() {
        let cfg = NetworkConfig::default();
        assert_eq!(cfg.mode, NetworkMode::Off);
        assert_eq!(cfg.public_ssid, WIFI_SSID);
        assert_eq!(cfg.gateway_ip, Ipv4Addr::new(10, 0, 0, 1));
    }

    #[tokio::test]
    async fn off_mode_returns_immediately() -> TestResult {
        let (_tx, rx) = watch::channel(RuntimeSignal::default());
        run(NetworkConfig::default(), rx).await?;
        Ok(())
    }

    #[test]
    fn plan_follows_signal() {
        let cfg = NetworkConfig::default();
        // Provisioning → SSID de setup.
        let setup = current_plan(&cfg, RuntimeSignal::default());
        assert_eq!(setup.kind, crate::plan::ApKind::Setup);
        // Emergency (avec ou sans alerte) → SSID public.
        for alert in [false, true] {
            let plan = current_plan(
                &cfg,
                RuntimeSignal {
                    installed: true,
                    alert_active: alert,
                },
            );
            assert_eq!(plan.ssid, WIFI_SSID);
            assert_eq!(plan.kind, crate::plan::ApKind::Public);
        }
    }
}
