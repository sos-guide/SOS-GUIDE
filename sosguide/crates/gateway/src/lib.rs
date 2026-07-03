//! Passerelle longue distance : **service caché Tor v3** à surface restreinte.
//!
//! Modèle « 3 canaux » de SOS-GUIDE : WiFi local (portail), LoRa (`sos-radio`)
//! et **Tor** (ce module, liens longue distance). Sur Tor, le nœud n'expose
//! qu'un **manifeste** ([`manifest`]) — identification + état d'alerte — servi
//! sur une adresse **loopback dédiée**, jamais le portail ni l'administration,
//! jamais la configuration complète. Le démon `tor` (externe) mappe le service
//! caché `.onion` vers ce port local via un `torrc` généré ([`torrc`]).
//!
//! # Modes ([`GatewayMode`], via `SOS_GW_MODE`)
//!
//! - **`off`** (défaut) : aucune tâche, aucun service exposé.
//! - **`simulate`** : sert le manifeste en HTTP sur le bind loopback, **sans
//!   Tor** — pour vérifier la surface localement (`curl`).
//! - **`live`** : `simulate` + génération du `torrc` et démarrage de `tor`.
//!   **Différé** : `tor` n'est pas installé sur le Pi de dev ; la génération du
//!   `torrc` est faite, le lancement du démon est journalisé comme à faire.

pub mod manifest;
pub mod torrc;

use std::net::SocketAddr;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use sos_core::RuntimeSignal;
use tokio::sync::watch;

/// Identité statique du nœud, injectée dans le manifeste.
#[derive(Debug, Clone)]
struct NodeIdentity {
    node_id: String,
    version: String,
}

/// État partagé du serveur de manifeste : identité + dernier signal runtime.
#[derive(Clone)]
struct ManifestState {
    identity: NodeIdentity,
    signal: watch::Receiver<RuntimeSignal>,
}

/// Mode d'exécution de la passerelle, dérivé de `SOS_GW_MODE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GatewayMode {
    /// Ne rien démarrer (défaut sûr).
    #[default]
    Off,
    /// Sert le manifeste en HTTP loopback, sans Tor.
    Simulate,
    /// Manifeste + `tor` (service caché réel). Différé (démon absent).
    Live,
}

impl GatewayMode {
    /// Interprète une valeur d'environnement ; toute valeur inconnue → `Off`.
    #[must_use]
    pub fn from_env_value(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "simulate" | "sim" => Self::Simulate,
            "live" => Self::Live,
            _ => Self::Off,
        }
    }
}

/// Configuration de la passerelle Tor.
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    /// Mode d'exécution.
    pub mode: GatewayMode,
    /// Adresse **loopback** où servir le manifeste (cible du service caché).
    pub manifest_bind: SocketAddr,
    /// Répertoire d'état du service caché (`HiddenServiceDir`).
    pub hs_dir: String,
    /// Identifiant du nœud (manifeste).
    pub node_id: String,
    /// Version du nœud (manifeste).
    pub version: String,
}

/// Erreur de la passerelle.
#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    /// Liaison du socket du manifeste impossible.
    #[error("liaison du manifeste sur {addr} impossible : {source}")]
    Bind {
        /// Adresse visée.
        addr: SocketAddr,
        /// Cause d'E/S.
        source: std::io::Error,
    },
    /// Service HTTP du manifeste interrompu.
    #[error("service du manifeste interrompu : {0}")]
    Serve(std::io::Error),
}

/// Handler du manifeste : reconstruit la projection publique à la volée depuis
/// le dernier signal runtime. Sert **uniquement** le manifeste (no-store).
async fn manifest_handler(State(state): State<ManifestState>) -> impl IntoResponse {
    let signal = *state.signal.borrow();
    let body = manifest::build(&state.identity.node_id, &state.identity.version, signal);
    (
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(body),
    )
}

/// Construit le routeur du manifeste : une **seule** route, en lecture seule.
fn manifest_router(state: ManifestState) -> Router {
    Router::new()
        .route("/", get(manifest_handler))
        .with_state(state)
}

/// Lance la passerelle.
///
/// `alerts` (cloné depuis le canal du portail) fournit la phase et l'état
/// d'alerte reflétés dans le manifeste. Retourne en mode `off`, ou quand le
/// service du manifeste s'arrête.
pub async fn run(
    cfg: GatewayConfig,
    alerts: watch::Receiver<RuntimeSignal>,
) -> Result<(), GatewayError> {
    if cfg.mode == GatewayMode::Off {
        tracing::info!("passerelle: mode off — aucun service exposé");
        return Ok(());
    }

    if cfg.mode == GatewayMode::Live {
        // Génère le torrc ; le lancement du démon tor est différé (absent du Pi).
        let conf = torrc::torrc(&cfg.hs_dir, cfg.manifest_bind);
        tracing::warn!(
            hs_dir = cfg.hs_dir,
            bytes = conf.len(),
            "passerelle: torrc généré — démarrage de tor différé (démon absent)"
        );
    }

    let state = ManifestState {
        identity: NodeIdentity {
            node_id: cfg.node_id.clone(),
            version: cfg.version.clone(),
        },
        signal: alerts,
    };
    let listener = tokio::net::TcpListener::bind(cfg.manifest_bind)
        .await
        .map_err(|source| GatewayError::Bind {
            addr: cfg.manifest_bind,
            source,
        })?;
    tracing::info!(addr = %cfg.manifest_bind, mode = ?cfg.mode, "passerelle: manifeste en écoute (loopback)");
    axum::serve(listener, manifest_router(state))
        .await
        .map_err(GatewayError::Serve)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn mode_parses_and_defaults_off() {
        assert_eq!(
            GatewayMode::from_env_value("simulate"),
            GatewayMode::Simulate
        );
        assert_eq!(GatewayMode::from_env_value(" LIVE "), GatewayMode::Live);
        assert_eq!(GatewayMode::from_env_value("zzz"), GatewayMode::Off);
    }

    #[tokio::test]
    async fn off_mode_returns_immediately() -> TestResult {
        let (_tx, rx) = watch::channel(RuntimeSignal::default());
        let cfg = GatewayConfig {
            mode: GatewayMode::Off,
            manifest_bind: ([127, 0, 0, 1], 0).into(),
            hs_dir: "/var/lib/tor/sos-guide".to_owned(),
            node_id: "n".to_owned(),
            version: "0.1.0".to_owned(),
        };
        run(cfg, rx).await?;
        Ok(())
    }

    #[tokio::test]
    async fn manifest_handler_serves_restricted_projection() -> TestResult {
        // Le routeur sert le manifeste construit depuis le signal courant.
        let (tx, rx) = watch::channel(RuntimeSignal {
            installed: true,
            alert_active: false,
        });
        let state = ManifestState {
            identity: NodeIdentity {
                node_id: "ecole-a".to_owned(),
                version: "0.1.0".to_owned(),
            },
            signal: rx,
        };
        let built = manifest::build("ecole-a", "0.1.0", *state.signal.borrow());
        assert_eq!(
            built.get("phase").and_then(|v| v.as_str()),
            Some("STATE_EMERGENCY")
        );
        drop(tx);
        let _ = manifest_router(state);
        Ok(())
    }
}
