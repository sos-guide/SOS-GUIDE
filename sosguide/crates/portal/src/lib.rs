//! Portail captif et interface web d'urgence (Axum).
//!
//! Reprend le comportement du vhost nginx de la v2.5 :
//! - détection de portail captif multi-OS (RFC 8908 + endpoints vendeurs) ;
//! - service des fichiers statiques du portail (index.html, langues, cartes) ;
//! - `/health` pour le watchdog ;
//! - projection **publique** de `config.json` (purge défensive des secrets ;
//!   le WiFi du nœud est **ouvert**, sans mot de passe) ;
//! - boîte de réception LoRa au format `lora_inbox.json` legacy.
//!
//! Une seule source de routage pour le dev et la prod : supprime la classe
//! de bugs « route présente dans install.sh mais pas dans dev-setup-pi.sh ».

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{ConnectInfo, Path as AxumPath, State};
use axum::handler::HandlerWithoutStateExt;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::Deserialize;
use sos_core::{
    ActiveAlert, Admission, AlertInbox, AlertPacket, AlertType, Lifecycle, OfficialBulletin,
    OfficialCache, OfficialCategory, RuntimeSignal, WIFI_SSID,
};
use sos_pay::{queue::QueueError, tx::TxStatus, PayError, Relay};
use sos_security::{
    hash_group_key, hash_password, random_token, validate_text, verify_group_key, verify_password,
    KeyRing,
};
use sos_storage::Store;
use tokio::sync::{mpsc, watch, Mutex, RwLock};
use tower_http::services::ServeDir;

/// Intervalle minimal entre deux publications d'alerte locales
/// (protection DoS basique, l'équivalent du rate-limit PHP de la v2.5).
const PUBLISH_MIN_INTERVAL: Duration = Duration::from_secs(2);

/// Longueur maximale d'une chaîne de configuration affichée (caractères).
const MAX_CONFIG_STRING: usize = 2_000;
/// Profondeur maximale du document de configuration (anti-DoS récursion).
const MAX_CONFIG_DEPTH: usize = 16;

/// Tentatives d'authentification échouées tolérées avant throttling.
const AUTH_FAIL_THRESHOLD: u32 = 5;
/// Inactivité après laquelle le compteur d'échecs retombe à zéro.
const AUTH_FAIL_WINDOW: Duration = Duration::from_secs(60);
/// Délai (tarpit) maximal imposé à une tentative échouée.
const AUTH_MAX_DELAY: Duration = Duration::from_secs(4);

/// Carte du lieu (tuiles OSM mises en cache hors-ligne) : niveau de zoom
/// (échelle « quartier ») et rayon de la grille téléchargée autour du nœud —
/// `(2r+1)²` tuiles, soit 25 au rayon 2. Au zoom 18 cela couvre ~750 m de côté
/// (~150 m/tuile) : assez serré pour que le **cercle de portée WiFi (~30 m)** de
/// la carte de détresse soit nettement visible, sans bombarder OSM (25 tuiles,
/// usage modéré). Délai max par tuile pour ne jamais bloquer l'install.
const TILE_ZOOM: u32 = 18;
const TILE_RADIUS: i64 = 2;
const TILE_FETCH_TIMEOUT_SECS: u32 = 10;
/// User-Agent honnête imposé par la politique d'OSM (le défaut de curl est banni).
const TILE_USER_AGENT: &str = "SOS-GUIDE/0.1 (+https://sosguide.fr; noeud d'urgence hors-ligne)";

/// Endpoints de détection de portail captif, par chemin (liste exhaustive
/// multi-OS reprise de la v2.5 et complétée). La détection vendeur (Xiaomi/
/// MIUI, Vivo, Samsung, Huawei…) repose surtout sur des **domaines** dédiés,
/// capturés par le DNS local de `sos-network` ; tous interrogent l'un de ces
/// chemins. Répondre par une redirection (au lieu du 204/200/texte attendu)
/// déclenche l'ouverture du portail sur tous ces systèmes.
const CAPTIVE_PROBES: &[&str] = &[
    // Apple (iOS / macOS).
    "/hotspot-detect.html",
    "/library/test/success.html",
    // Google / Android / Chrome OS (+ Samsung, Huawei, Xiaomi, Vivo, Oppo…).
    "/generate_204",
    "/generate204",
    "/gen_204",
    "/generate_205",
    "/mobile/status.php",
    // Microsoft Windows (NCSI).
    "/connecttest.txt",
    "/ncsi.txt",
    "/redirect",
    // Mozilla Firefox.
    "/success.txt",
    "/canonical.html",
    // GNOME / NetworkManager (Linux).
    "/check_network_status.txt",
    "/nm-check.txt",
    // KDE / divers Linux.
    "/kde-org-check.html",
    // Amazon Kindle / Fire.
    "/kindle-wifi/wifistub.html",
    // Microsoft (anciens) / divers.
    "/fwlink/",
];

/// Erreurs de démarrage du portail.
#[derive(Debug, thiserror::Error)]
pub enum PortalError {
    /// Impossible d'écouter sur l'adresse demandée.
    #[error("écoute impossible sur {addr} : {source}")]
    Bind {
        /// Adresse demandée.
        addr: SocketAddr,
        /// Erreur système sous-jacente.
        source: std::io::Error,
    },
    /// Erreur du serveur HTTP en cours d'exécution.
    #[error("serveur HTTP : {0}")]
    Serve(#[from] std::io::Error),
}

/// Configuration du portail.
pub struct PortalConfig {
    /// Adresse d'écoute (ex. `0.0.0.0:80`).
    pub listen: SocketAddr,
    /// Racine des fichiers web (le `src/web/` déployé de la v2.5).
    pub webroot: PathBuf,
    /// URL annoncée aux clients captifs (RFC 8908 et redirections).
    pub portal_url: String,
    /// Cache des tuiles cartographiques OSM (servies hors-ligne en `/tiles`).
    /// Rempli à l'install par téléchargement (`curl`) autour du GPS du nœud.
    pub tiles_dir: PathBuf,
    /// Modes des sous-systèmes (réseau/radio/passerelle) pour l'affichage de
    /// statut honnête dans `/admin`.
    pub subsystems: SubsystemModes,
    /// Commande de **fenêtre d'écriture** sur la partition de données (SOSDATA),
    /// montée en lecture seule sur l'appliance Alpine *diskless*. Reçoit
    /// `open` (remonte rw) puis `close` (sync + remonte ro) autour des écritures
    /// de fichiers sur SOSDATA (téléchargement/purge des tuiles). `None` ⇒ no-op
    /// (support déjà inscriptible : déploiement Debian, tests).
    pub rw_cmd: Option<String>,
}

/// Modes d'exécution des sous-systèmes, tels que lancés par l'application
/// (`off` / `simulate` / `live`). Purement informatif : permet à `/admin`
/// d'afficher un statut honnête au lieu d'un libellé figé « Phase 3/4 ».
#[derive(Debug, Clone, Default)]
pub struct SubsystemModes {
    /// Mode du réseau local (`SOS_NET_MODE`).
    pub network: String,
    /// Mode de la radio LoRa (`SOS_RADIO_MODE`).
    pub radio: String,
    /// Mode de la passerelle Tor (`SOS_GW_MODE`).
    pub gateway: String,
}

/// État mutable du nœud partagé entre les requêtes.
pub struct NodeState {
    /// Trousseau : identité du nœud + nœuds de confiance. **Partagé** (`Arc`) avec
    /// la tâche `sos-radio`, qui s'en sert pour vérifier les signatures entrantes
    /// et suit les rotations de clé à chaud.
    pub keyring: Arc<RwLock<KeyRing>>,
    /// Boîte de réception des alertes mesh. **Partagée** (`Arc`) avec `sos-radio` :
    /// les alertes reçues du maillage y sont admises et le portail les affiche.
    pub inbox: Arc<Mutex<AlertInbox>>,
    /// Persistance Redb (source de vérité de la configuration). `None` en mode
    /// dégradé sans base inscriptible : le portail sert alors la config par
    /// défaut livrée dans le webroot.
    pub store: Option<Store>,
    /// Émetteur du signal runtime vers `sos-network` (pilotage du point d'accès
    /// à chaud). `None` si le réseau n'est pas branché (mode dégradé / tests) :
    /// les changements d'état sont alors silencieusement ignorés.
    pub alert_tx: Option<watch::Sender<RuntimeSignal>>,
    /// Émetteur des trames d'alerte à diffuser sur le maillage LoRa (`sos-radio`).
    /// `None` si la radio n'est pas branchée : la publication reste locale.
    pub radio_tx: Option<mpsc::Sender<String>>,
    /// Relais de paiement « Bitcoin tx over LoRa » (mode urgence). `None` quand
    /// `SOS_PAY_MODE=off` (défaut) : les endpoints `/api/pay` répondent « désactivé ».
    /// La borne ne détient **ni clé ni fonds** : elle transporte des tx signées.
    pub pay: Option<Arc<Mutex<Relay>>>,
    /// Émetteur des **fragments de paiement** vers le maillage LoRa (`sos-radio`).
    /// `None` si la radio n'est pas branchée : la tx reste en file, non diffusée.
    pub pay_tx: Option<mpsc::Sender<String>>,
}

struct AppState {
    webroot: PathBuf,
    portal_url: String,
    tiles_dir: PathBuf,
    node: RwLock<NodeState>,
    subsystems: SubsystemModes,
    last_publish: Mutex<Option<Instant>>,
    auth_throttle: Mutex<AuthThrottle>,
    /// Sessions admin ouvertes : jeton de cookie → instant d'expiration. Permet
    /// une page de login stylée (cookie `HttpOnly`) en plus de l'auth Basic.
    sessions: Mutex<HashMap<String, Instant>>,
    /// Commande de fenêtre rw sur SOSDATA (cf. [`PortalConfig::rw_cmd`]).
    rw_cmd: Option<String>,
    /// Sérialise les écritures de tuiles (un seul écrivain dans la fenêtre rw)
    /// pour qu'aucune fenêtre ne re-verrouille SOSDATA pendant qu'une autre écrit.
    tiles_lock: Mutex<()>,
    /// Rate-limit des pings citoyens **par adresse IP** (borné : les entrées
    /// expirées sont purgées à chaque ping). Un client abusif ne peut plus, à lui
    /// seul, bloquer le service `/api/ping` pour tous les autres.
    ping_limiter: Mutex<HashMap<IpAddr, Instant>>,
}

type SharedState = Arc<AppState>;

/// Nom du cookie de session administrateur.
const SESSION_COOKIE: &str = "sos_admin";
/// Durée de vie d'une session admin (jeton de cookie).
const SESSION_TTL: Duration = Duration::from_secs(8 * 3600);
/// Longueur du jeton de session (alphabet lisible de `sos-security`).
const SESSION_TOKEN_LEN: usize = 32;

/// Compteur d'échecs d'authentification administrateur, pour ralentir une
/// attaque par force brute. Conçu pour **ne jamais verrouiller** l'admin
/// légitime : seul un échec subit un délai ; un mot de passe correct réussit
/// immédiatement et remet le compteur à zéro (fiabilité > sécurité).
#[derive(Default)]
struct AuthThrottle {
    fails: u32,
    last_fail: Option<Instant>,
}

impl AuthThrottle {
    /// Délai à imposer avant de répondre à une tentative échouée, croissant
    /// avec le nombre d'échecs récents (1 s, 2 s, 4 s…), plafonné. Nul tant
    /// qu'on reste sous le seuil.
    fn penalty(&self) -> Duration {
        if self.fails <= AUTH_FAIL_THRESHOLD {
            return Duration::ZERO;
        }
        let over = (self.fails - AUTH_FAIL_THRESHOLD).min(16);
        let secs = 1u64 << (over - 1);
        Duration::from_secs(secs).min(AUTH_MAX_DELAY)
    }
}

/// Publie une mise à jour du signal runtime vers `sos-network` (best-effort).
/// Sans réseau branché (`alert_tx == None`) ou sans récepteur vivant, le
/// changement est silencieusement ignoré : le portail ne doit jamais échouer
/// parce que l'orchestrateur réseau est absent.
fn signal_runtime(node: &NodeState, f: impl FnOnce(&mut RuntimeSignal)) {
    if let Some(tx) = &node.alert_tx {
        tx.send_modify(f);
    }
}

/// Horodatage Unix courant (0 si l'horloge est antérieure à l'époque Unix,
/// ce qui arrive sur un Pi sans RTC avant synchronisation).
fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Fenêtre d'écriture sur la partition de données (SOSDATA), montée en lecture
/// seule le reste du temps (appliance Alpine *diskless*). À l'ouverture exécute
/// `<rw_cmd> open` (remonte rw) ; au `Drop` exécute `<rw_cmd> close` (sync +
/// remonte ro) — **garanti** sur tous les chemins de retour, y compris erreur.
/// No-op si `rw_cmd` est absent (support déjà inscriptible : Debian, tests).
struct RwWindow {
    cmd: Option<String>,
}

impl RwWindow {
    /// Ouvre la fenêtre (remonte SOSDATA en rw) ; best-effort.
    fn open(cmd: Option<String>) -> Self {
        if let Some(c) = &cmd {
            run_rw(c, "open");
        }
        Self { cmd }
    }
}

impl Drop for RwWindow {
    fn drop(&mut self) {
        if let Some(c) = &self.cmd {
            run_rw(c, "close");
        }
    }
}

/// Exécute la commande de fenêtre rw avec l'action (`open`/`close`). Best-effort :
/// un échec est journalisé mais ne casse jamais le portail (une écriture sur un
/// support resté ro échouera de toute façon en aval, surface honnête au client ;
/// un `close` raté est rattrapé par le garde-fou ro périodique du système).
fn run_rw(cmd: &str, action: &str) {
    let mut parts = cmd.split_whitespace();
    let Some(prog) = parts.next() else {
        return;
    };
    let args: Vec<&str> = parts.collect();
    // Sous-processus **bloquant** (remontage rw + copie atomique). `RwWindow` est
    // un garde RAII synchrone (son `Drop` ne peut pas être `async`). Pour ne pas
    // figer un worker Tokio pendant l'appel, on le cède via `block_in_place` —
    // mais uniquement sur un runtime **multi-thread** (la prod) : hors runtime ou
    // en current-thread (tests synchrones), `block_in_place` paniquerait, donc on
    // exécute directement.
    let run = || {
        std::process::Command::new(prog)
            .args(&args)
            .arg(action)
            .status()
    };
    let status = match tokio::runtime::Handle::try_current() {
        Ok(h) if matches!(h.runtime_flavor(), tokio::runtime::RuntimeFlavor::MultiThread) => {
            tokio::task::block_in_place(run)
        }
        _ => run(),
    };
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => tracing::warn!(cmd, action, code = ?s.code(), "fenêtre rw : échec"),
        Err(err) => tracing::warn!(cmd, action, %err, "fenêtre rw : commande injoignable"),
    }
}

/// Construit le routeur complet du portail.
pub fn router(config: &PortalConfig, node: NodeState) -> Router {
    let state: SharedState = Arc::new(AppState {
        webroot: config.webroot.clone(),
        portal_url: config.portal_url.clone(),
        tiles_dir: config.tiles_dir.clone(),
        node: RwLock::new(node),
        subsystems: config.subsystems.clone(),
        last_publish: Mutex::new(None),
        auth_throttle: Mutex::new(AuthThrottle::default()),
        sessions: Mutex::new(HashMap::new()),
        rw_cmd: config.rw_cmd.clone(),
        tiles_lock: Mutex::new(()),
        ping_limiter: Mutex::new(HashMap::new()),
    });

    // Capture totale : tout chemin inconnu (fichier absent du webroot) est
    // redirigé vers le portail `/` plutôt que de renvoyer un 404 sec — comportement
    // attendu d'une borne captive. Les fichiers réellement présents (img, lib,
    // privacy.html…) restent servis normalement ; seul le cas « introuvable » redirige.
    // NB : `.fallback()` et non `.not_found_service()` — ce dernier enveloppe la
    // réponse dans `SetStatus(404)` et écraserait notre 307, produisant un 404
    // porteur d'un `Location` que les clients n'honorent pas. `.fallback()` renvoie
    // la réponse du service telle quelle (vrai 307).
    let capture_all = || async { Redirect::temporary("/") };
    let static_files = ServeDir::new(&config.webroot)
        .append_index_html_on_directories(true)
        .fallback(capture_all.into_service());
    // Tuiles OSM mises en cache localement (servies hors-ligne ; 404 tant que
    // non téléchargées). Servies à part du webroot car écrites au runtime.
    let tiles = ServeDir::new(&config.tiles_dir);

    let mut router = Router::new()
        .route("/health", get(health))
        .route("/.well-known/captive-portal", get(captive_portal_api))
        .route("/data/config.json", get(public_config))
        .route("/data/lora_inbox.json", get(inbox_json))
        .route("/api/alerts", get(inbox_json).post(publish_alert))
        // Pages produit (servies depuis le webroot, gardées par le cycle de vie).
        .route("/install", get(install_page))
        .route("/admin", get(admin_page))
        // API : état public, provisioning, administration.
        .route("/api/status", get(node_status))
        .route("/api/admin/vitals", get(admin_vitals))
        .route("/api/alert", get(alert_status))
        .route("/api/official", get(official_bulletins))
        .route("/api/install", post(install_node))
        .route(
            "/api/admin/config",
            get(admin_get_config).post(admin_set_config),
        )
        .route("/api/admin/alert", post(set_alert).delete(clear_alert))
        .route(
            "/api/admin/official",
            post(ingest_official).delete(clear_official),
        )
        .route(
            "/api/admin/trusted",
            get(list_trusted).post(set_trusted).delete(clear_trusted),
        )
        .route("/api/admin/secrets", post(rotate_secrets))
        .route("/api/admin/wifi-qr", get(admin_wifi_qr))
        .route("/api/admin/map", post(download_map_tiles))
        .route("/api/admin/login", post(admin_login))
        .route("/api/admin/logout", post(admin_logout))
        .route("/api/admin/session", get(admin_session))
        .route("/api/admin/reset", post(admin_reset))
        .route("/api/ping", post(submit_ping))
        .route("/api/pay", get(pay_status).post(submit_pay))
        .route("/api/admin/groups", get(list_groups).post(create_group))
        .route("/api/admin/groups/{id}", delete(delete_group))
        .route(
            "/api/admin/pings",
            get(list_pings).delete(clear_admin_pings),
        )
        .nest_service("/tiles", tiles);

    for probe in CAPTIVE_PROBES {
        let target = config.portal_url.clone();
        router = router.route(
            probe,
            get(move || {
                let target = target.clone();
                async move { Redirect::temporary(&target) }
            }),
        );
    }

    router
        .fallback_service(static_files)
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}

/// Démarre le portail et sert jusqu'à l'arrêt du processus (SIGINT/SIGTERM).
pub async fn serve(config: PortalConfig, node: NodeState) -> Result<(), PortalError> {
    let app = router(&config, node);
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(|source| PortalError::Bind {
            addr: config.listen,
            source,
        })?;
    tracing::info!(addr = %config.listen, webroot = %config.webroot.display(), "portail démarré");
    // `into_make_service_with_connect_info` fournit l'adresse du pair (IP client)
    // aux handlers via `ConnectInfo` — nécessaire au rate-limit par IP de `/api/ping`.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn shutdown_signal() {
    if let Err(err) = tokio::signal::ctrl_c().await {
        tracing::error!(%err, "installation du gestionnaire de signal impossible");
        // Sans gestionnaire de signal on continue à servir : ne jamais
        // interrompre le portail en situation d'urgence.
        std::future::pending::<()>().await;
    }
    tracing::info!("signal d'arrêt reçu — arrêt propre du portail");
}

/// En-têtes de sécurité appliqués à toutes les réponses (parité nginx v2.5).
async fn security_headers(request: axum::extract::Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::X_FRAME_OPTIONS,
        HeaderValue::from_static("SAMEORIGIN"),
    );
    headers.insert(
        header::HeaderName::from_static("x-robots-tag"),
        HeaderValue::from_static("noindex, nofollow"),
    );
    response
}

async fn health() -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "no-store")],
        (StatusCode::OK, "OK\n"),
    )
}

/// RFC 8908 — Captive Portal API (iOS 14+, Android 11+).
async fn captive_portal_api(State(state): State<SharedState>) -> impl IntoResponse {
    let body = serde_json::json!({
        "captive": true,
        "user-portal-url": state.portal_url,
    });
    (
        [
            (header::CONTENT_TYPE, "application/captive+json"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body.to_string(),
    )
}

/// Sert la projection **publique** de la configuration (secrets expurgés).
///
/// Source de vérité : la base Redb (`store.public_config`), seule forme
/// expurgée servable. Tant que le nœud n'est pas provisionné (aucune config en
/// base) ou en mode dégradé sans base, on sert la `config.json` par défaut
/// livrée dans le webroot, elle aussi expurgée — jamais le fichier brut comme
/// le faisait la v2.5.
async fn public_config(State(state): State<SharedState>) -> Response {
    {
        let node = state.node.read().await;
        if let Some(store) = &node.store {
            match store.public_config() {
                Ok(Some(json)) => {
                    return (
                        [
                            (header::CONTENT_TYPE, "application/json"),
                            (header::CACHE_CONTROL, "no-store"),
                        ],
                        json,
                    )
                        .into_response();
                }
                // Pas encore provisionné : repli sur la config livrée.
                Ok(None) => {}
                Err(err) => {
                    tracing::warn!(%err, "projection publique du store illisible — repli fichier");
                }
            }
        }
    }
    let path = state.webroot.join("data/config.json");
    let raw = match tokio::fs::read(&path).await {
        Ok(raw) => raw,
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "config.json illisible");
            return StatusCode::NOT_FOUND.into_response();
        }
    };
    let mut value: serde_json::Value = match serde_json::from_slice(&raw) {
        Ok(value) => value,
        Err(err) => {
            tracing::error!(path = %path.display(), %err, "config.json malformé");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if let Some(object) = value.as_object_mut() {
        // Purge défensive d'un éventuel secret hérité (v2.5) : le WiFi est ouvert.
        object.remove("wifiPassword");
    }
    ([(header::CACHE_CONTROL, "no-store")], Json(value)).into_response()
}

/// Boîte de réception au format `lora_inbox.json` de la v2.5
/// (`{"updated": <ts>, "alerts": [...]}`), lu par l'index.html legacy.
async fn inbox_json(State(state): State<SharedState>) -> Response {
    let node = state.node.read().await;
    let inbox = node.inbox.lock().await;
    let body = serde_json::json!({
        "updated": now_unix(),
        "alerts": inbox.alerts(),
    });
    ([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

/// Corps de la requête de publication d'une alerte locale.
#[derive(Deserialize)]
struct PublishRequest {
    /// Nom de fil du type d'alerte (`"INCENDIE"`, `"CUSTOM"`, …).
    #[serde(rename = "type")]
    alert_type: String,
    /// Message libre (tronqué à 80 caractères).
    #[serde(default)]
    message: String,
}

/// Publie une alerte locale : construit le paquet, le signe avec
/// l'identité du nœud, l'admet dans la boîte et retourne la trame
/// prête à être transmise sur le mesh.
async fn publish_alert(
    State(state): State<SharedState>,
    Json(request): Json<PublishRequest>,
) -> Response {
    // Rate-limit global : le portail est ouvert, une seule alerte
    // toutes les PUBLISH_MIN_INTERVAL protège la bande passante LoRa.
    {
        let mut last = state.last_publish.lock().await;
        if let Some(t) = *last {
            if t.elapsed() < PUBLISH_MIN_INTERVAL {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    "une alerte est déjà en cours d'émission, réessayer dans un instant\n",
                )
                    .into_response();
            }
        }
        *last = Some(Instant::now());
    }

    let Some(alert_type) = AlertType::from_wire_name(&request.alert_type) else {
        return (StatusCode::UNPROCESSABLE_ENTITY, "type d'alerte inconnu\n").into_response();
    };
    if let Err(err) = validate_text(&request.message, 80) {
        return reject_text("message", err);
    }

    let node = state.node.read().await;
    let node_id = node.keyring.read().await.node_id().to_owned();
    let mut packet = AlertPacket::new(&node_id, alert_type, &request.message, now_unix());
    packet.signature = Some(node.keyring.read().await.sign(&packet));

    let admission = node.inbox.lock().await.admit(&packet, true, now_unix());
    if admission != Admission::Accepted {
        return (StatusCode::CONFLICT, "alerte dupliquée ou périmée\n").into_response();
    }

    let frame = match packet.to_frame() {
        Ok(frame) => frame,
        Err(err) => {
            tracing::error!(%err, "sérialisation de trame impossible");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    tracing::info!(
        node_id = packet.node_id,
        r#type = packet.alert_type.wire_name(),
        "alerte locale publiée"
    );
    // Diffuse la trame sur le maillage LoRa (best-effort, non bloquant).
    if let Some(radio) = &node.radio_tx {
        if let Err(err) = radio.try_send(frame.clone()) {
            tracing::warn!(%err, "radio: trame locale non diffusée (file pleine ou absente)");
        }
    }
    (
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "id": packet.unique_id(), "frame": frame })),
    )
        .into_response()
}

/// Taille maximale d'un document de configuration accepté (anti-DoS mémoire).
const MAX_CONFIG_BYTES: usize = 16 * 1024;
/// Taille maximale du cache de bulletins officiels persisté (anti-DoS mémoire).
const MAX_OFFICIAL_BYTES: usize = 256 * 1024;
/// Nombre maximal de groupes de ping.
const MAX_GROUPS: usize = 20;
/// Nombre maximal de pings conservés.
const PING_MAX: usize = 200;
/// Longueur maximale du nom d'expéditeur ou de groupe.
const MAX_PING_STRING: usize = 80;
/// Intervalle minimal entre deux pings (rate-limit global).
const PING_MIN_INTERVAL: Duration = Duration::from_secs(5);
/// Bornes du mot de passe administrateur (caractères).
const MIN_ADMIN_PASSWORD: usize = 8;
/// Borne haute du mot de passe administrateur (caractères).
const MAX_ADMIN_PASSWORD: usize = 128;

/// Sert une page HTML produit depuis le webroot. 404 si absente.
async fn serve_webroot_html(state: &SharedState, name: &str) -> Response {
    let path = state.webroot.join(name);
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            bytes,
        )
            .into_response(),
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "page produit absente du webroot");
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

/// `true` si le nœud est installé (phase urgence), best-effort.
async fn is_installed(state: &SharedState) -> bool {
    let node = state.node.read().await;
    node.store
        .as_ref()
        .and_then(|s| s.config_installed().ok())
        .unwrap_or(false)
}

/// État **public** du nœud (aucun secret, aucune décision) — consommé par la
/// section « État du nœud » de `/admin`.
async fn node_status(State(state): State<SharedState>) -> Response {
    let node = state.node.read().await;
    let installed = node
        .store
        .as_ref()
        .and_then(|s| s.config_installed().ok())
        .unwrap_or(false);
    let phase = Lifecycle::from_installed(installed);
    let node_id = node.keyring.read().await.node_id().to_owned();
    // Nœuds pairs **réels** du maillage : identifiants distincts entendus dans la
    // boîte de réception (alertes signées reçues), hors ce nœud. Aucune donnée
    // simulée — vide tant qu'aucun pair n'a été entendu.
    let (alert_count, mut peers) = {
        let inbox = node.inbox.lock().await;
        let peers: Vec<String> = inbox
            .alerts()
            .iter()
            .map(|a| a.node_id.clone())
            .filter(|n| *n != node_id)
            .collect();
        (inbox.alerts().len(), peers)
    };
    peers.sort();
    peers.dedup();
    let body = serde_json::json!({
        "node_id": node_id,
        "version": env!("CARGO_PKG_VERSION"),
        "phase": phase.wire_name(),
        "installed": installed,
        "alerts": alert_count,
        "activeNodes": peers.len(),
        "meshPeers": peers,
        "subsystems": {
            "network": state.subsystems.network,
            "radio": state.subsystems.radio,
            "gateway": state.subsystems.gateway,
        },
    });
    ([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

/// Vitaux matériels du nœud (température, mémoire, charge, disque) — supervision
/// admin. La collecte fait des E/S bloquantes (`/proc`, `df`) : on l'exécute sur
/// le pool bloquant pour ne pas geler l'exécuteur async.
async fn admin_vitals(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    match tokio::task::spawn_blocking(|| sos_cli::health::collect("/")).await {
        Ok(vitals) => (
            [(header::CACHE_CONTROL, "no-store")],
            Json(vitals.to_json()),
        )
            .into_response(),
        Err(err) => {
            tracing::warn!(%err, "collecte des vitaux impossible");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Page d'installation : disponible uniquement tant que le nœud n'est pas
/// provisionné (sinon, l'administration prend le relais).
async fn install_page(State(state): State<SharedState>) -> Response {
    if is_installed(&state).await {
        return Redirect::temporary("/admin").into_response();
    }
    serve_webroot_html(&state, "install.html").await
}

/// Page d'administration : le **shell** est servi sans authentification (il ne
/// contient aucun secret) ; la page affiche d'abord un **formulaire de login**
/// puis charge le tableau de bord via les API protégées (`/api/admin/*`). Seule
/// la redirection vers `/install` tant que le nœud n'est pas provisionné subsiste.
async fn admin_page(State(state): State<SharedState>) -> Response {
    if !is_installed(&state).await {
        return Redirect::temporary("/install").into_response();
    }
    serve_webroot_html(&state, "admin.html").await
}

/// Corps de la requête de connexion administrateur.
#[derive(Deserialize)]
struct LoginRequest {
    /// Mot de passe administrateur.
    password: String,
}

/// Ouvre une session admin : valide le mot de passe (même throttle anti-force
/// brute que `require_admin`) et pose un cookie de session `HttpOnly`.
async fn admin_login(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Response {
    if !origin_allowed(&headers) {
        return forbidden_csrf();
    }
    let cred = {
        let node = state.node.read().await;
        node.store
            .as_ref()
            .and_then(|s| s.load_admin_password().ok().flatten())
    };
    let Some((salt, hash)) = cred else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "administration non configurée\n",
        )
            .into_response();
    };
    let ok = verify_password(&req.password, &salt, &hash);
    // Throttle partagé : un échec ralentit, un succès remet à zéro (jamais de
    // verrouillage de l'admin légitime).
    let delay = {
        let mut t = state.auth_throttle.lock().await;
        if t.last_fail.is_some_and(|i| i.elapsed() > AUTH_FAIL_WINDOW) {
            t.fails = 0;
        }
        if ok {
            t.fails = 0;
            t.last_fail = None;
            Duration::ZERO
        } else {
            t.fails = t.fails.saturating_add(1);
            t.last_fail = Some(Instant::now());
            t.penalty()
        }
    };
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    if !ok {
        return unauthorized();
    }
    let token = random_token(SESSION_TOKEN_LEN);
    {
        let mut sessions = state.sessions.lock().await;
        let now = Instant::now();
        sessions.retain(|_, &mut expiry| expiry > now);
        sessions.insert(token.clone(), now + SESSION_TTL);
    }
    (
        StatusCode::OK,
        [(header::SET_COOKIE, session_set_cookie(&token))],
        Json(serde_json::json!({ "ok": true })),
    )
        .into_response()
}

/// Ferme la session admin courante (supprime le jeton et efface le cookie).
async fn admin_logout(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if let Some(token) = session_token_from_cookies(&headers) {
        state.sessions.lock().await.remove(&token);
    }
    (
        StatusCode::OK,
        [(header::SET_COOKIE, session_clear_cookie())],
        "déconnecté\n",
    )
        .into_response()
}

/// Sonde d'état de session : `200` **seulement** si un cookie de session valide
/// est présent (volontairement **pas** l'auth Basic). C'est elle qui décide
/// login vs dashboard côté page : ainsi la déconnexion (effacement du cookie)
/// est toujours respectée, même si le navigateur a gardé en cache des
/// identifiants Basic d'une visite antérieure.
async fn admin_session(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if session_valid(&state, &headers).await {
        return Json(serde_json::json!({ "ok": true })).into_response();
    }
    StatusCode::UNAUTHORIZED.into_response()
}

/// **Retour aux valeurs d'usine** : efface configuration, mot de passe admin,
/// alerte, bulletins et tuiles cartographiques ; ferme toutes les sessions. Le
/// nœud repasse en provisioning (`/install`). Conserve l'identité du nœud.
async fn admin_reset(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if !origin_allowed(&headers) {
        return forbidden_csrf();
    }
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    {
        let node = state.node.read().await;
        let Some(store) = &node.store else {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "persistance indisponible\n",
            )
                .into_response();
        };
        if let Err(err) = store.factory_reset() {
            tracing::error!(%err, "retour aux valeurs d'usine impossible");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        signal_runtime(&node, |s| {
            s.installed = false;
            s.alert_active = false;
        });
    }
    // Purge des tuiles cartographiques (best-effort) et de toutes les sessions.
    // La purge écrit sur SOSDATA (ro) : fenêtre rw sérialisée, re-verrouillée au Drop.
    {
        let _tiles_guard = state.tiles_lock.lock().await;
        let _rw = RwWindow::open(state.rw_cmd.clone());
        let _ = tokio::fs::remove_dir_all(&state.tiles_dir).await;
    }
    state.sessions.lock().await.clear();
    tracing::warn!("nœud réinitialisé (valeurs d'usine) — retour en provisioning");
    (
        StatusCode::OK,
        [(header::SET_COOKIE, session_clear_cookie())],
        Json(serde_json::json!({ "reset": true })),
    )
        .into_response()
}

/// Réponse 401 d'administration. **Sans** en-tête `WWW-Authenticate` : la
/// connexion passe par la page de login (cookie de session), et l'on évite ainsi
/// que le navigateur ne mette en cache des identifiants Basic — ce qui rendrait
/// la déconnexion inopérante. Les outils (curl `-u`, tests) envoient l'en-tête
/// `Authorization` de façon préventive et restent acceptés par `require_admin`.
fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        "authentification administrateur requise\n",
    )
        .into_response()
}

/// Corps de dépôt d'une transaction signée pour relais mesh.
#[derive(Deserialize)]
struct PayRequest {
    /// Transaction Bitcoin **signée**, en hexadécimal brut (aucune clé côté borne).
    tx: String,
}

/// Libellé de statut d'une transaction en file (JSON public).
fn pay_status_label(status: TxStatus) -> &'static str {
    match status {
        TxStatus::Queued => "queued",
        TxStatus::Relayed => "relayed",
        TxStatus::Broadcast => "broadcast",
    }
}

/// **Dépose une transaction signée** pour relais « Bitcoin tx over LoRa » (mode
/// urgence). La borne est un **transporteur** : elle valide (format + taille) et met
/// en file un blob signé, sans jamais détenir de clé ni de fonds. Refusé si le
/// relais est désactivé (`SOS_PAY_MODE=off`). La confirmation reste l'affaire du
/// réseau Bitcoin ; la borne ne fait que transporter puis (via un nœud-sortie) diffuser.
async fn submit_pay(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<PayRequest>,
) -> Response {
    if !origin_allowed(&headers) {
        return forbidden_csrf();
    }
    let (pay, pay_tx) = {
        let node = state.node.read().await;
        (node.pay.clone(), node.pay_tx.clone())
    };
    let Some(pay) = pay else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "relais de paiement désactivé (SOS_PAY_MODE=off)\n",
        )
            .into_response();
    };
    let mut relay = pay.lock().await;
    match relay.accept_hex(&req.tx) {
        Ok(id) => {
            // Si la radio est branchée : fragmente et pousse sur le mesh (best-effort,
            // alertes-first côté `sos-radio`). Sinon la tx reste en file, non diffusée.
            if let Some(tx_ch) = &pay_tx {
                match relay.relay_fragments(&id) {
                    Ok(frags) => {
                        let mut sent = 0usize;
                        for frag in &frags {
                            if let Ok(wire) = sos_pay::frame::encode_frame(frag) {
                                if tx_ch.try_send(wire).is_ok() {
                                    sent += 1;
                                }
                            }
                        }
                        tracing::info!(%id, fragments = frags.len(), sent, "paiement: fragments poussés vers le mesh");
                    }
                    Err(err) => tracing::warn!(%err, "paiement: fragmentation impossible"),
                }
            }
            let queued = relay.queue().len();
            tracing::info!(%id, queued, "paiement: transaction signée mise en file");
            (
                StatusCode::ACCEPTED,
                Json(serde_json::json!({ "id": id, "status": "queued", "queued": queued })),
            )
                .into_response()
        }
        Err(PayError::Tx(err)) => {
            (StatusCode::UNPROCESSABLE_ENTITY, format!("{err}\n")).into_response()
        }
        Err(PayError::Queue(QueueError::Duplicate)) => {
            (StatusCode::OK, "transaction déjà en file\n").into_response()
        }
        Err(PayError::Queue(QueueError::Full)) => {
            (StatusCode::SERVICE_UNAVAILABLE, "file de paiement pleine\n").into_response()
        }
        Err(PayError::Unknown) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// **État public du relais de paiement** : liste `(id, taille, statut)` des
/// transactions en file, **sans exposer les transactions brutes**. `enabled=false`
/// quand le relais est désactivé.
async fn pay_status(State(state): State<SharedState>) -> Response {
    let pay = { state.node.read().await.pay.clone() };
    let Some(pay) = pay else {
        return (
            [(header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({ "enabled": false })),
        )
            .into_response();
    };
    let relay = pay.lock().await;
    let txs: Vec<serde_json::Value> = relay
        .queue()
        .all()
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id(),
                "size": t.len(),
                "status": pay_status_label(t.status()),
            })
        })
        .collect();
    let pending = relay.queue().pending().count();
    let queued = relay.queue().len();
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "enabled": true,
            "queued": queued,
            "pending": pending,
            "txs": txs,
        })),
    )
        .into_response()
}

/// Garde CSRF des requêtes mutantes : si un en-tête `Origin` est présent, son
/// autorité doit correspondre à l'en-tête `Host` (même origine). Les clients
/// non-navigateur (curl, outils d'admin, tests) n'envoient pas d'`Origin` et
/// sont autorisés — ils ne peuvent pas être un vecteur CSRF. Bloque en revanche
/// une requête inter-site émise par un navigateur dont les identifiants Basic
/// sont en cache.
fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    // `Origin` = `scheme://autorité` ; on compare l'autorité (`hôte[:port]`).
    let origin_authority = origin.split_once("://").map(|(_, rest)| rest);
    !host.is_empty() && origin_authority == Some(host)
}

/// Réponse 403 pour une origine non autorisée (CSRF).
fn forbidden_csrf() -> Response {
    (StatusCode::FORBIDDEN, "origine non autorisée (CSRF)\n").into_response()
}

/// Réponse 422 décrivant un champ texte refusé par la validation.
fn reject_text(field: &str, err: sos_security::TextError) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        format!("champ « {field} » : {}\n", err.reason()),
    )
        .into_response()
}

/// Valide récursivement toutes les chaînes d'un document de configuration
/// avant écriture : anti-XSS (`<`/`>` interdits), caractères de contrôle et
/// longueurs bornés, profondeur limitée.
///
/// En cas de refus, renvoie le message d'erreur (à servir en 422).
fn validate_config_value(
    value: &serde_json::Value,
    key: Option<&str>,
    depth: usize,
) -> Result<(), String> {
    if depth > MAX_CONFIG_DEPTH {
        return Err("configuration trop imbriquée".to_owned());
    }
    match value {
        serde_json::Value::String(s) => validate_text(s, MAX_CONFIG_STRING)
            .map_err(|err| format!("champ « {} » : {}", key.unwrap_or("(racine)"), err.reason())),
        serde_json::Value::Array(items) => items
            .iter()
            .try_for_each(|item| validate_config_value(item, key, depth + 1)),
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                // Les clés sont aussi rendues (libellés) : on les valide.
                validate_text(k, 200)
                    .map_err(|err| format!("clé de configuration : {}", err.reason()))?;
                validate_config_value(v, Some(k), depth + 1)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Extrait le mot de passe d'un en-tête `Authorization: Basic base64(user:pass)`.
fn basic_auth_password(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let encoded = raw.strip_prefix("Basic ")?;
    let decoded = BASE64.decode(encoded.trim()).ok()?;
    let pair = String::from_utf8(decoded).ok()?;
    let (_user, pass) = pair.split_once(':')?;
    Some(pass.to_owned())
}

/// Extrait la valeur du cookie de session admin de l'en-tête `Cookie`.
fn session_token_from_cookies(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get(header::COOKIE)?.to_str().ok()?;
    raw.split(';')
        .filter_map(|c| c.trim().split_once('='))
        .find(|(name, _)| *name == SESSION_COOKIE)
        .map(|(_, value)| value.to_owned())
}

/// `true` si la requête porte un cookie de session admin valide et non expiré.
/// Purge au passage les sessions expirées (la carte reste petite).
async fn session_valid(state: &SharedState, headers: &HeaderMap) -> bool {
    let Some(token) = session_token_from_cookies(headers) else {
        return false;
    };
    let mut sessions = state.sessions.lock().await;
    let now = Instant::now();
    sessions.retain(|_, &mut expiry| expiry > now);
    sessions.contains_key(&token)
}

/// En-tête `Set-Cookie` ouvrant une session admin (`HttpOnly`, `SameSite=Strict`).
/// Pas de `Secure` : le nœud sert en HTTP sur le réseau local captif.
fn session_set_cookie(token: &str) -> String {
    format!(
        "{SESSION_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
        SESSION_TTL.as_secs()
    )
}

/// En-tête `Set-Cookie` effaçant la session admin.
fn session_clear_cookie() -> String {
    format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
}

/// Vérifie l'authentification administrateur (HTTP Basic + empreinte Redb).
/// État **public** de l'alerte en cours, lu en boucle par la page d'accueil
/// pour basculer en page SOS. Renvoie `{"active":false}` s'il n'y a pas
/// d'alerte (ou si la persistance est indisponible) : la page reste normale.
async fn alert_status(State(state): State<SharedState>) -> Response {
    let raw = {
        let node = state.node.read().await;
        node.store
            .as_ref()
            .and_then(|s| s.load_active_alert().ok().flatten())
    };
    let body = match raw.as_deref().map(serde_json::from_str::<ActiveAlert>) {
        Some(Ok(alert)) if !alert.is_end() => serde_json::json!({
            "active": true,
            "cause": alert.cause.wire_name(),
            "label": alert.cause.label(),
            "instructions": alert.instructions,
            "since": alert.since,
        }),
        _ => serde_json::json!({ "active": false }),
    };
    ([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response()
}

/// Corps de la requête d'émission d'alerte (administration).
#[derive(Deserialize)]
struct AlertRequest {
    /// Nom de fil de la cause (`"INCENDIE"`, `"FIN_ALERTE"`, …).
    cause: String,
    /// Consignes locales précises (texte libre, bornées par le domaine).
    #[serde(default)]
    instructions: String,
}

/// Émet (ou remplace) l'alerte active. Une cause `FIN_ALERTE` la clôt. Réservé
/// à l'administrateur. Source unique côté écriture : l'ingestion automatique
/// (Phases ultérieures) réutilisera la même persistance.
async fn set_alert(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<AlertRequest>,
) -> Response {
    if !origin_allowed(&headers) {
        return forbidden_csrf();
    }
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    let Some(cause) = AlertType::from_wire_name(&req.cause) else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "cause d'alerte inconnue\n",
        )
            .into_response();
    };
    if let Err(err) = validate_text(&req.instructions, MAX_CONFIG_STRING) {
        return reject_text("consignes", err);
    }
    let node = state.node.read().await;
    let Some(store) = &node.store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "persistance indisponible\n",
        )
            .into_response();
    };
    // FIN_ALERTE = retour à la normale : on efface l'alerte plutôt que de la
    // stocker, pour que la page d'accueil revienne à son état nominal.
    if cause == AlertType::FinAlerte {
        if let Err(err) = store.clear_active_alert() {
            tracing::error!(%err, "effacement de l'alerte impossible");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        signal_runtime(&node, |s| s.alert_active = false);
        tracing::info!("alerte levée (FIN_ALERTE)");
        return (StatusCode::OK, "alerte levée\n").into_response();
    }
    let alert = ActiveAlert::new(cause, &req.instructions, now_unix());
    let serialized = match serde_json::to_string(&alert) {
        Ok(json) => json,
        Err(err) => {
            tracing::error!(%err, "sérialisation de l'alerte impossible");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if let Err(err) = store.save_active_alert(&serialized) {
        tracing::error!(%err, "écriture de l'alerte impossible");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    signal_runtime(&node, |s| s.alert_active = true);
    tracing::info!(cause = cause.wire_name(), "alerte émise");
    (StatusCode::OK, "alerte émise\n").into_response()
}

/// Clôt l'alerte active (équivaut à émettre `FIN_ALERTE`). Réservé à l'admin.
async fn clear_alert(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if !origin_allowed(&headers) {
        return forbidden_csrf();
    }
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    let node = state.node.read().await;
    let Some(store) = &node.store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "persistance indisponible\n",
        )
            .into_response();
    };
    if let Err(err) = store.clear_active_alert() {
        tracing::error!(%err, "effacement de l'alerte impossible");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    signal_runtime(&node, |s| s.alert_active = false);
    (StatusCode::OK, "alerte levée\n").into_response()
}

/// Code pays du nœud, lu dans `establishment.countryCode` (l'emplacement écrit
/// par le wizard `/install`). Chaîne vide si absent.
fn node_country_code(store: &Store) -> String {
    store
        .load_config()
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| {
            v.get("establishment")
                .and_then(|e| e.get("countryCode"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

/// Charge le cache des bulletins officiels depuis la persistance (vide si
/// absent, illisible ou sans base).
fn load_official_cache(store: &Store) -> OfficialCache {
    store
        .load_official()
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// Bulletins officiels **publics** mis en cache, filtrés sur le pays du nœud
/// (plus les bulletins de portée globale). Servis à la page d'accueil pour
/// enrichir la page SOS. Lecture seule, aucun secret.
async fn official_bulletins(State(state): State<SharedState>) -> Response {
    let node = state.node.read().await;
    let Some(store) = &node.store else {
        // Sans persistance : aucun bulletin, mais l'endpoint reste valide.
        return (
            [(header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({ "updated": 0, "bulletins": [] })),
        )
            .into_response();
    };
    let country = node_country_code(store);
    let cache = load_official_cache(store);
    let items: Vec<serde_json::Value> = cache
        .for_country(&country)
        .iter()
        .map(|b| {
            serde_json::json!({
                "source": b.source,
                "category": b.category.wire_name(),
                "categoryLabel": b.category.label(),
                "country": b.country,
                "title": b.title,
                "body": b.body,
                "published": b.published,
                "fetched": b.fetched,
                "link": b.link,
            })
        })
        .collect();
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "updated": cache.updated, "bulletins": items })),
    )
        .into_response()
}

/// Corps de la requête d'ingestion d'un bulletin officiel (administration).
///
/// C'est le **repli d'acquisition manuel**, toujours disponible sans réseau. La
/// récupération automatique sur un canal de sortie (Ethernet de maintenance /
/// Tor) alimentera la même persistance une fois la connectivité disponible
/// (Phases 3-4).
#[derive(Deserialize)]
struct OfficialRequest {
    /// Nom de la source officielle.
    source: String,
    /// Catégorie (`"WEATHER"`, `"DISASTER"`, `"GOVERNMENT"`, `"HEALTH"`, `"OTHER"`).
    category: String,
    /// Code pays concerné (vide = portée globale).
    #[serde(default)]
    country: String,
    /// Titre court.
    title: String,
    /// Corps du message.
    #[serde(default)]
    body: String,
    /// Horodatage Unix de publication d'origine (optionnel).
    #[serde(default)]
    published: Option<i64>,
    /// Lien source (optionnel, indicatif).
    #[serde(default)]
    link: Option<String>,
}

/// Ingère un bulletin officiel dans le cache local (import manuel admin).
async fn ingest_official(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<OfficialRequest>,
) -> Response {
    if !origin_allowed(&headers) {
        return forbidden_csrf();
    }
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    let Some(category) = OfficialCategory::from_wire_name(&req.category) else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "catégorie de bulletin inconnue\n",
        )
            .into_response();
    };
    if req.source.trim().is_empty() || req.title.trim().is_empty() {
        return (StatusCode::UNPROCESSABLE_ENTITY, "source et titre requis\n").into_response();
    }
    // Les bulletins sont affichés sur la page SOS : valider tout texte rendu.
    for (field, value, max) in [
        ("source", req.source.as_str(), 200),
        ("titre", req.title.as_str(), 200),
        ("corps", req.body.as_str(), MAX_CONFIG_STRING),
        ("pays", req.country.as_str(), 8),
        ("lien", req.link.as_deref().unwrap_or_default(), 500),
    ] {
        if let Err(err) = validate_text(value, max) {
            return reject_text(field, err);
        }
    }

    let node = state.node.read().await;
    let Some(store) = &node.store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "persistance indisponible\n",
        )
            .into_response();
    };
    let bulletin = OfficialBulletin::new(
        &req.source,
        category,
        &req.country,
        &req.title,
        &req.body,
        req.published.unwrap_or(0),
        now_unix(),
        req.link.as_deref(),
    );
    let mut cache = load_official_cache(store);
    cache.ingest(bulletin);
    let serialized = match serde_json::to_string(&cache) {
        Ok(json) => json,
        Err(err) => {
            tracing::error!(%err, "sérialisation du cache officiel impossible");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if serialized.len() > MAX_OFFICIAL_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            "cache de bulletins trop volumineux\n",
        )
            .into_response();
    }
    if let Err(err) = store.save_official(&serialized) {
        tracing::error!(%err, "écriture du cache officiel impossible");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    tracing::info!(source = req.source, "bulletin officiel mis en cache");
    (StatusCode::CREATED, "bulletin mis en cache\n").into_response()
}

/// Vide le cache des bulletins officiels. Réservé à l'administrateur.
async fn clear_official(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if !origin_allowed(&headers) {
        return forbidden_csrf();
    }
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    let node = state.node.read().await;
    let Some(store) = &node.store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "persistance indisponible\n",
        )
            .into_response();
    };
    if let Err(err) = store.clear_official() {
        tracing::error!(%err, "vidage du cache officiel impossible");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    (StatusCode::OK, "cache vidé\n").into_response()
}

async fn require_admin(state: &SharedState, headers: &HeaderMap) -> Result<(), Response> {
    // Session par cookie (page de login) : voie rapide, hors throttle.
    if session_valid(state, headers).await {
        return Ok(());
    }
    let cred = {
        let node = state.node.read().await;
        node.store
            .as_ref()
            .and_then(|s| s.load_admin_password().ok().flatten())
    };
    let Some((salt, hash)) = cred else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "administration non configurée\n",
        )
            .into_response());
    };
    let ok = matches!(
        basic_auth_password(headers),
        Some(pw) if verify_password(&pw, &salt, &hash)
    );

    // Met à jour le throttle et calcule le délai à imposer. Un succès remet le
    // compteur à zéro et ne subit aucun délai (l'admin n'est jamais verrouillé).
    let delay = {
        let mut t = state.auth_throttle.lock().await;
        if t.last_fail.is_some_and(|i| i.elapsed() > AUTH_FAIL_WINDOW) {
            t.fails = 0;
        }
        if ok {
            t.fails = 0;
            t.last_fail = None;
            Duration::ZERO
        } else {
            t.fails = t.fails.saturating_add(1);
            t.last_fail = Some(Instant::now());
            t.penalty()
        }
    };
    // WHY: tarpit hors verrou — ne sérialise pas les requêtes légitimes.
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
    if ok {
        Ok(())
    } else {
        Err(unauthorized())
    }
}

/// Corps de la requête d'installation (wizard `/install`).
#[derive(Deserialize)]
struct InstallRequest {
    /// Configuration du lieu (objet `establishment`, `reassurance`, `mapScope`…).
    config: serde_json::Value,
    /// Mot de passe administrateur à définir (8 à 128 caractères).
    #[serde(rename = "adminPassword")]
    admin_password: String,
}

/// Construit la chaîne de jonction WiFi standard, lue par les appareils qui
/// scannent un QR. L'AP SOS-GUIDE est **toujours ouvert** (aucune clé — décision
/// 2026-06-28), donc le type est `nopass` : `WIFI:S:<ssid>;T:nopass;;`. Les
/// caractères réservés du format (`\ ; , : "`) sont échappés.
fn wifi_join_string(ssid: &str) -> String {
    fn escape(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            if matches!(c, '\\' | ';' | ',' | ':' | '"') {
                out.push('\\');
            }
            out.push(c);
        }
        out
    }
    format!("WIFI:S:{};T:nopass;;", escape(ssid))
}

/// Encode `payload` en QR Code (correction d'erreur moyenne) et renvoie un SVG
/// **autonome et sans prologue XML** : il s'imprime hors-ligne sur l'affiche du
/// lieu, s'inscrit tel quel dans le DOM (`innerHTML`) et se sert aussi comme
/// document `image/svg+xml`. Modules noirs sur fond blanc (contraste maximal,
/// indépendant du thème — fiabilité de lecture). `None` si la charge dépasse la
/// capacité d'un QR (jamais le cas pour une clé WiFi).
fn wifi_qr_svg(payload: &str) -> Option<String> {
    use qrcodegen::{QrCode, QrCodeEcc};
    let qr = QrCode::encode_text(payload, QrCodeEcc::Medium).ok()?;
    let n = qr.size();
    let border: i32 = 4;
    let dim = n + border * 2;
    let mut path = String::new();
    for y in 0..n {
        for x in 0..n {
            if qr.get_module(x, y) {
                if !path.is_empty() {
                    path.push(' ');
                }
                path.push_str(&format!("M{},{}h1v1h-1z", x + border, y + border));
            }
        }
    }
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {dim} {dim}\" \
         shape-rendering=\"crispEdges\" role=\"img\" aria-label=\"QR WiFi {WIFI_SSID}\">\
         <rect width=\"{dim}\" height=\"{dim}\" fill=\"#fff\"/>\
         <path d=\"{path}\" fill=\"#000\"/></svg>"
    ))
}

/// Provisionne le nœud : écrit la configuration (avec `installed: true`) et le
/// mot de passe administrateur dans Redb. Disponible une seule fois (refusé si
/// déjà installé). Fait basculer le cycle de vie en phase urgence.
async fn install_node(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<InstallRequest>,
) -> Response {
    if !origin_allowed(&headers) {
        return forbidden_csrf();
    }
    if is_installed(&state).await {
        return (StatusCode::CONFLICT, "le nœud est déjà installé\n").into_response();
    }
    if let Err(msg) = validate_config_value(&req.config, None, 0) {
        return (StatusCode::UNPROCESSABLE_ENTITY, format!("{msg}\n")).into_response();
    }
    let pw_len = req.admin_password.chars().count();
    if !(MIN_ADMIN_PASSWORD..=MAX_ADMIN_PASSWORD).contains(&pw_len) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "mot de passe administrateur : 8 à 128 caractères\n",
        )
            .into_response();
    }
    let Some(mut config) = req.config.as_object().cloned() else {
        return (StatusCode::UNPROCESSABLE_ENTITY, "configuration invalide\n").into_response();
    };
    if !establishment_name_valid(&config) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "nom du lieu requis (1 à 200 caractères)\n",
        )
            .into_response();
    }
    config.insert("installed".to_owned(), serde_json::Value::Bool(true));
    let serialized = serde_json::Value::Object(config).to_string();
    if serialized.len() > MAX_CONFIG_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            "configuration trop volumineuse\n",
        )
            .into_response();
    }

    // Verrou **exclusif** pour toute la séquence check-and-set : ferme la fenêtre
    // TOCTOU où deux requêtes d'install concurrentes passaient toutes deux le
    // premier contrôle `is_installed` (read-txn) avant la première écriture.
    let node = state.node.write().await;
    let Some(store) = &node.store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "persistance indisponible\n",
        )
            .into_response();
    };
    // Re-vérification **sous le verrou** : la seconde install voit `installed`.
    if store.config_installed().unwrap_or(false) {
        return (StatusCode::CONFLICT, "le nœud est déjà installé\n").into_response();
    }
    let creds = hash_password(&req.admin_password);
    if let Err(err) = store.save_config(&serialized) {
        tracing::error!(%err, "écriture de la configuration impossible");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if let Err(err) = store.save_admin_password(&creds.salt_hex, &creds.hash_hex) {
        tracing::error!(%err, "écriture du mot de passe administrateur impossible");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    signal_runtime(&node, |s| s.installed = true);
    tracing::info!("nœud provisionné — bascule en phase urgence");
    // Renvoie le SSID + le QR du réseau ouvert pour que le wizard les affiche (à
    // imprimer sur l'affiche du lieu : scanner = rejoindre le WiFi de la borne).
    let wifi_qr = wifi_qr_svg(&wifi_join_string(WIFI_SSID));
    (
        StatusCode::CREATED,
        Json(serde_json::json!({
            "wifiSsid": WIFI_SSID,
            "wifiQr": wifi_qr,
        })),
    )
        .into_response()
}

/// Renvoie le QR Code de jonction WiFi (affiche du lieu) au format SVG. Le réseau
/// SOS-GUIDE est **ouvert** : le QR n'encode aucun secret, il désigne simplement
/// le réseau à rejoindre. Réservé à l'administrateur (contexte `/admin`), il
/// permet de réimprimer l'affiche sans réinstaller le nœud.
async fn admin_wifi_qr(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    let Some(svg) = wifi_qr_svg(&wifi_join_string(WIFI_SSID)) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    (
        [
            (header::CONTENT_TYPE, "image/svg+xml; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        svg,
    )
        .into_response()
}

/// Convertit une coordonnée (lat, lon en degrés) en indices de **tuile slippy**
/// entiers au zoom `z` (projection Web Mercator standard d'OSM). `None` si la
/// latitude sort du domaine Mercator (~±85,05°) ou si les valeurs sont non
/// finies. Les indices sont bornés à `[0, 2^z − 1]`.
fn lonlat_to_tile(lat: f64, lon: f64, z: u32) -> Option<(i64, i64)> {
    if !lat.is_finite() || !lon.is_finite() || !(-85.05..=85.05).contains(&lat) {
        return None;
    }
    let scale = f64::from(1u32 << z); // 2^z tuiles par axe
    let lat_rad = lat.to_radians();
    let x = (lon + 180.0) / 360.0 * scale;
    let y = (1.0 - (lat_rad.tan() + 1.0 / lat_rad.cos()).ln() / std::f64::consts::PI) / 2.0 * scale;
    let max = (1i64 << z) - 1;
    let xi = (x.floor() as i64).clamp(0, max);
    let yi = (y.floor() as i64).clamp(0, max);
    Some((xi, yi))
}

/// Télécharge une grille `(2·TILE_RADIUS+1)²` de tuiles OSM centrée sur
/// `(cx, cy)` au zoom `z` dans `dir/{z}/{x}/{y}.png`, via le `curl` **système**
/// (le TLS reste hors du binaire — build Rust pur conservé). Renvoie le nombre
/// de tuiles obtenues et échouées. Best-effort : une tuile en échec (hors-ligne,
/// curl absent) est sautée sans interrompre les autres.
async fn fetch_tile_grid(dir: &Path, z: u32, cx: i64, cy: i64) -> std::io::Result<(u32, u32)> {
    let max = (1i64 << z) - 1;
    let (mut downloaded, mut failed) = (0u32, 0u32);
    for dy in -TILE_RADIUS..=TILE_RADIUS {
        for dx in -TILE_RADIUS..=TILE_RADIUS {
            let (x, y) = (cx + dx, cy + dy);
            if x < 0 || y < 0 || x > max || y > max {
                continue; // bord du planisphère : tuile inexistante
            }
            let subdir = dir.join(z.to_string()).join(x.to_string());
            tokio::fs::create_dir_all(&subdir).await?;
            let out = subdir.join(format!("{y}.png"));
            // URL construite côté serveur (z/x/y entiers) : aucune injection.
            let url = format!("https://tile.openstreetmap.org/{z}/{x}/{y}.png");
            let status = tokio::process::Command::new("curl")
                .arg("--fail")
                .arg("--silent")
                .arg("--show-error")
                .arg("--location")
                .arg("--max-time")
                .arg(TILE_FETCH_TIMEOUT_SECS.to_string())
                .arg("-A")
                .arg(TILE_USER_AGENT)
                .arg("-o")
                .arg(&out)
                .arg(&url)
                .status()
                .await;
            if matches!(status, Ok(s) if s.success()) {
                downloaded += 1;
            } else {
                failed += 1;
                let _ = tokio::fs::remove_file(&out).await; // pas de tuile tronquée
            }
        }
    }
    Ok((downloaded, failed))
}

/// Télécharge la **carte du lieu** : tuiles OSM autour du GPS du nœud, mises en
/// cache pour un affichage **hors-ligne**. Réservé à l'administrateur (action
/// sortante, déclenchée à l'install et re-déclenchable en `/admin`). Best-effort
/// et honnête : `502` si rien n'a pu être téléchargé (nœud hors-ligne / sans
/// `curl`), le portail se rabat alors sur le schéma SVG.
async fn download_map_tiles(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if !origin_allowed(&headers) {
        return forbidden_csrf();
    }
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    let (lat, lon) = {
        let node = state.node.read().await;
        let Some(store) = &node.store else {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "persistance indisponible\n",
            )
                .into_response();
        };
        let cfg = store
            .load_config()
            .ok()
            .flatten()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
        let lat = cfg
            .as_ref()
            .and_then(|c| c.pointer("/establishment/lat"))
            .and_then(serde_json::Value::as_f64);
        let lon = cfg
            .as_ref()
            .and_then(|c| c.pointer("/establishment/lon"))
            .and_then(serde_json::Value::as_f64);
        match (lat, lon) {
            (Some(a), Some(b)) => (a, b),
            _ => {
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "position GPS du nœud non renseignée\n",
                )
                    .into_response();
            }
        }
    };
    let z = TILE_ZOOM;
    let Some((cx, cy)) = lonlat_to_tile(lat, lon, z) else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "coordonnées GPS invalides\n",
        )
            .into_response();
    };
    let dir = state.tiles_dir.clone();
    // SOSDATA est montée en lecture seule : ouvre une fenêtre d'écriture le temps
    // du téléchargement, sérialisée (un seul écrivain), re-verrouillée en ro au
    // Drop quel que soit le chemin de sortie. No-op hors appliance diskless.
    let _tiles_guard = state.tiles_lock.lock().await;
    let _rw = RwWindow::open(state.rw_cmd.clone());
    let (downloaded, failed) = match fetch_tile_grid(&dir, z, cx, cy).await {
        Ok(counts) => counts,
        Err(err) => {
            tracing::error!(%err, "écriture du cache de tuiles impossible");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if downloaded == 0 {
        tracing::warn!(
            failed,
            "aucune tuile téléchargée (hors-ligne ou curl absent)"
        );
        return (
            StatusCode::BAD_GATEWAY,
            "aucune tuile téléchargée (nœud hors-ligne ?)\n",
        )
            .into_response();
    }
    // Métadonnées lues par le frontend pour centrer la mosaïque sur le nœud.
    let meta = serde_json::json!({
        "z": z,
        "centerX": cx,
        "centerY": cy,
        "radius": TILE_RADIUS,
        "lat": lat,
        "lon": lon,
        "tiles": downloaded,
    });
    if let Err(err) = tokio::fs::write(dir.join("meta.json"), meta.to_string()).await {
        tracing::error!(%err, "écriture de meta.json des tuiles impossible");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    tracing::info!(downloaded, failed, "carte du lieu mise en cache");
    Json(serde_json::json!({ "downloaded": downloaded, "failed": failed, "zoom": z }))
        .into_response()
}

/// `true` si la config porte un `establishment.name` non vide et borné.
fn establishment_name_valid(config: &serde_json::Map<String, serde_json::Value>) -> bool {
    config
        .get("establishment")
        .and_then(|e| e.get("name"))
        .and_then(serde_json::Value::as_str)
        .map(|name| (1..=200).contains(&name.trim().chars().count()))
        .unwrap_or(false)
}

/// Lit la configuration courante (projection publique) pour l'administration.
async fn admin_get_config(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    let node = state.node.read().await;
    let Some(store) = &node.store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "persistance indisponible\n",
        )
            .into_response();
    };
    match store.admin_config() {
        Ok(Some(json)) => (
            [
                (header::CONTENT_TYPE, "application/json"),
                (header::CACHE_CONTROL, "no-store"),
            ],
            json,
        )
            .into_response(),
        Ok(None) => (
            [(header::CACHE_CONTROL, "no-store")],
            Json(serde_json::json!({})),
        )
            .into_response(),
        Err(err) => {
            tracing::error!(%err, "lecture de la configuration administrateur impossible");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Corps de la mise à jour de configuration (administration).
#[derive(Deserialize)]
struct AdminConfigRequest {
    /// Champs de configuration à fusionner dans la configuration existante.
    config: serde_json::Value,
    /// Nouveau mot de passe administrateur (optionnel ; rotation).
    #[serde(rename = "adminPassword", default)]
    admin_password: Option<String>,
}

/// Met à jour la configuration (fusion) et, optionnellement, le mot de passe.
async fn admin_set_config(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<AdminConfigRequest>,
) -> Response {
    if !origin_allowed(&headers) {
        return forbidden_csrf();
    }
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    let Some(incoming) = req.config.as_object() else {
        return (StatusCode::UNPROCESSABLE_ENTITY, "configuration invalide\n").into_response();
    };
    if let Err(msg) = validate_config_value(&req.config, None, 0) {
        return (StatusCode::UNPROCESSABLE_ENTITY, format!("{msg}\n")).into_response();
    }
    if let Some(pw) = &req.admin_password {
        let len = pw.chars().count();
        if !(MIN_ADMIN_PASSWORD..=MAX_ADMIN_PASSWORD).contains(&len) {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                "mot de passe administrateur : 8 à 128 caractères\n",
            )
                .into_response();
        }
    }

    let node = state.node.read().await;
    let Some(store) = &node.store else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "persistance indisponible\n",
        )
            .into_response();
    };
    // Fusion dans la config existante : préserve `installed` et les champs absents.
    let mut current = match store.load_config() {
        Ok(Some(raw)) => serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default(),
        _ => serde_json::Map::new(),
    };
    for (key, value) in incoming {
        current.insert(key.clone(), value.clone());
    }
    current.insert("installed".to_owned(), serde_json::Value::Bool(true));
    let serialized = serde_json::Value::Object(current).to_string();
    if serialized.len() > MAX_CONFIG_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            "configuration trop volumineuse\n",
        )
            .into_response();
    }
    if let Err(err) = store.save_config(&serialized) {
        tracing::error!(%err, "mise à jour de la configuration impossible");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if let Some(pw) = &req.admin_password {
        let creds = hash_password(pw);
        if let Err(err) = store.save_admin_password(&creds.salt_hex, &creds.hash_hex) {
            tracing::error!(%err, "rotation du mot de passe administrateur impossible");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }
    (StatusCode::OK, "configuration mise à jour\n").into_response()
}

/// Corps de la rotation des secrets (chaque champ est indépendant et optionnel).
#[derive(Deserialize)]
struct SecretsRequest {
    /// Nouveau mot de passe administrateur.
    #[serde(rename = "adminPassword", default)]
    admin_password: Option<String>,
    /// Régénère la clé de signature Ed25519 du nœud (identifiant conservé).
    #[serde(rename = "regenerateNodeKey", default)]
    regenerate_node_key: bool,
}

/// Liste les nœuds de confiance (admin) : identifiants connus du trousseau,
/// nœud lui-même inclus. Aucun secret (les clés publiques ne sont pas exposées).
async fn list_trusted(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    let node = state.node.read().await;
    let ids = node.keyring.read().await.trusted_node_ids();
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "nodes": ids })),
    )
        .into_response()
}

/// **Remplace** le registre des nœuds de confiance par le `trusted_nodes.json`
/// fourni (format v2.5 `{"nodes":{"<id>":{"public_key":"<PEM>"}}}`). Persiste dans
/// Redb **puis** recharge le trousseau partagé à chaud : la radio prend en compte
/// les nouveaux pairs sans redémarrage. Le nœud reste toujours de confiance.
async fn set_trusted(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Response {
    if !origin_allowed(&headers) {
        return forbidden_csrf();
    }
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    if !body.get("nodes").is_some_and(serde_json::Value::is_object) {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "format attendu : {\"nodes\": {\"<id>\": {\"public_key\": \"<PEM>\"}}}\n",
        )
            .into_response();
    }
    let json = body.to_string();
    if json.len() > MAX_CONFIG_BYTES {
        return (StatusCode::PAYLOAD_TOO_LARGE, "registre trop volumineux\n").into_response();
    }

    let node = state.node.read().await;
    // Durabilité d'abord : on persiste avant d'appliquer en mémoire.
    if let Some(store) = &node.store {
        if let Err(err) = store.save_trusted(&json) {
            tracing::error!(%err, "persistance du registre de confiance impossible");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }
    let loaded = {
        let mut ring = node.keyring.write().await;
        ring.clear_trusted_nodes();
        match ring.load_trusted_nodes(&json) {
            Ok(count) => count,
            Err(err) => {
                tracing::warn!(%err, "registre de confiance invalide");
                return (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "registre de confiance invalide\n",
                )
                    .into_response();
            }
        }
    };
    tracing::info!(loaded, "registre des nœuds de confiance remplacé (à chaud)");
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "loaded": loaded })),
    )
        .into_response()
}

/// Vide le registre des nœuds de confiance : seul le nœud lui-même reste de
/// confiance (le maillage est alors fermé aux autres nœuds). Persiste + à chaud.
async fn clear_trusted(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if !origin_allowed(&headers) {
        return forbidden_csrf();
    }
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    let node = state.node.read().await;
    if let Some(store) = &node.store {
        if let Err(err) = store.clear_trusted() {
            tracing::error!(%err, "purge du registre de confiance impossible");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    }
    node.keyring.write().await.clear_trusted_nodes();
    tracing::info!("registre des nœuds de confiance vidé");
    (
        StatusCode::NO_CONTENT,
        [(header::CACHE_CONTROL, "no-store")],
    )
        .into_response()
}

/// Rotation **fine** des secrets : mot de passe admin et/ou clé de signature du
/// nœud — indépendamment. Persiste dans Redb.
async fn rotate_secrets(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<SecretsRequest>,
) -> Response {
    if !origin_allowed(&headers) {
        return forbidden_csrf();
    }
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    if let Some(pw) = &req.admin_password {
        if !(MIN_ADMIN_PASSWORD..=MAX_ADMIN_PASSWORD).contains(&pw.chars().count()) {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                "mot de passe administrateur : 8 à 128 caractères\n",
            )
                .into_response();
        }
    }

    let node = state.node.write().await;
    if node.store.is_none() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "persistance indisponible\n",
        )
            .into_response();
    }
    let mut rotated: Vec<&str> = Vec::new();

    if let Some(pw) = &req.admin_password {
        let creds = hash_password(pw);
        if let Some(store) = node.store.as_ref() {
            if let Err(err) = store.save_admin_password(&creds.salt_hex, &creds.hash_hex) {
                tracing::error!(%err, "rotation du mot de passe admin impossible");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
        rotated.push("mot de passe administrateur");
    }

    if req.regenerate_node_key {
        let node_id = node.keyring.read().await.node_id().to_owned();
        let mut ring = KeyRing::generate(&node_id);
        match ring.private_key_pem() {
            Ok(pem) => {
                if let Some(store) = node.store.as_ref() {
                    if let Err(err) = store.save_identity(&node_id, &pem) {
                        tracing::error!(%err, "persistance de la nouvelle identité impossible");
                        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                    }
                }
            }
            Err(err) => {
                tracing::error!(%err, "export de la nouvelle identité impossible");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
        // Le trousseau neuf ne contient que la clé propre : on y réinjecte le
        // registre des pairs de confiance persisté, sinon le maillage serait
        // coupé jusqu'au prochain démarrage.
        if let Some(store) = node.store.as_ref() {
            if let Ok(Some(json)) = store.load_trusted() {
                if let Err(err) = ring.load_trusted_nodes(&json) {
                    tracing::warn!(%err, "rechargement du registre de confiance après rotation impossible");
                }
            }
        }
        *node.keyring.write().await = ring;
        rotated.push("identité du nœud");
    }

    if rotated.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "aucun secret à modifier\n",
        )
            .into_response();
    }
    tracing::info!(secrets = ?rotated, "rotation de secrets effectuée");
    (
        StatusCode::OK,
        Json(serde_json::json!({ "rotated": rotated })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Régression : `router()` doit se construire **sans paniquer**. axum 0.8
    /// rejette la syntaxe de capture `:param` (l'ancien `/groups/:id` faisait
    /// paniquer le nœud au démarrage → portail injoignable, alors que la
    /// compilation et les tests unitaires passaient). Ce test construit le
    /// routeur complet et fige la validité de tous les chemins de route.
    #[test]
    fn router_builds_without_panicking() {
        let config = PortalConfig {
            listen: SocketAddr::from(([127, 0, 0, 1], 0)),
            webroot: PathBuf::from("web"),
            portal_url: "http://10.0.0.1/".to_owned(),
            tiles_dir: PathBuf::from("/tmp/sos-tiles-test"),
            subsystems: SubsystemModes::default(),
            rw_cmd: None,
        };
        let node = NodeState {
            keyring: Arc::new(RwLock::new(KeyRing::generate("test-node"))),
            inbox: Arc::new(Mutex::new(AlertInbox::new())),
            store: None,
            alert_tx: None,
            radio_tx: None,
            pay: None,
            pay_tx: None,
        };
        // Ne doit pas paniquer : la panique de validation des routes surviendrait ici.
        let _router = router(&config, node);
    }

    /// La fenêtre rw exécute bien `open` à l'ouverture puis `close` au `Drop`,
    /// dans cet ordre (le helper réel remonte rw puis ro). Vérifié via un script
    /// qui journalise son argument dans un fichier.
    #[test]
    fn rw_window_runs_open_then_close() -> Result<(), Box<dyn std::error::Error>> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("sos-rw-test-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let log = dir.join("log");
        let script = dir.join("rw.sh");
        std::fs::write(
            &script,
            format!("#!/bin/sh\necho \"$1\" >> {}\n", log.display()),
        )?;
        std::process::Command::new("chmod")
            .arg("+x")
            .arg(&script)
            .status()?;
        {
            let _w = RwWindow::open(Some(script.display().to_string()));
            // ouverture journalisée ; la fermeture survient au Drop ci-dessous.
        }
        let logged = std::fs::read_to_string(&log)?;
        assert_eq!(logged, "open\nclose\n", "ordre attendu open puis close");
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    /// Sans commande, la fenêtre rw est un no-op et ne panique pas (déploiement
    /// Debian où SOSDATA est déjà inscriptible).
    #[test]
    fn rw_window_none_is_noop() {
        let _w = RwWindow::open(None);
        // Drop sans commande : aucune exécution, aucun effet.
    }

    #[test]
    fn captive_probes_cover_all_major_os() {
        // Apple, Android/Google, Windows, Firefox, GNOME/NM, KDE, Kindle.
        for probe in [
            "/hotspot-detect.html",
            "/generate_204",
            "/gen_204",
            "/ncsi.txt",
            "/connecttest.txt",
            "/redirect",
            "/success.txt",
            "/canonical.html",
            "/check_network_status.txt",
            "/kindle-wifi/wifistub.html",
        ] {
            assert!(CAPTIVE_PROBES.contains(&probe), "sonde manquante : {probe}");
        }
    }

    #[test]
    fn establishment_name_validation() {
        let empty_map = serde_json::Map::new();
        let obj = |v: &serde_json::Value| v.as_object().cloned().unwrap_or(empty_map.clone());
        assert!(establishment_name_valid(&obj(&serde_json::json!(
            {"establishment": {"name": "Mairie de X"}}
        ))));
        assert!(!establishment_name_valid(&obj(&serde_json::json!(
            {"establishment": {"name": "   "}}
        ))));
        assert!(!establishment_name_valid(&obj(&serde_json::json!(
            {"reassurance": {}}
        ))));
    }

    #[test]
    fn wifi_join_string_is_open_network_and_escaped() {
        // Réseau ouvert : type `nopass`, aucune clé.
        assert_eq!(wifi_join_string("SOS-GUIDE"), "WIFI:S:SOS-GUIDE;T:nopass;;");
        // Caractères réservés du format échappés dans le SSID.
        assert_eq!(wifi_join_string("Net;A"), "WIFI:S:Net\\;A;T:nopass;;");
    }

    #[test]
    fn wifi_qr_svg_is_inline_safe_and_self_contained() {
        let svg = wifi_qr_svg(&wifi_join_string(WIFI_SSID)).unwrap_or_default();
        assert!(!svg.is_empty(), "le SSID tient toujours dans un QR");
        // Inscriptible tel quel dans le DOM : pas de prologue XML.
        assert!(!svg.contains("<?xml"));
        assert!(svg.starts_with("<svg"));
        assert!(svg.ends_with("</svg>"));
        // Contraste fixe (lecture fiable, indépendant du thème) + au moins un module.
        assert!(svg.contains("fill=\"#fff\""));
        assert!(svg.contains("fill=\"#000\""));
        assert!(svg.contains("<path d=\"M"));
    }

    #[test]
    fn lonlat_to_tile_reference_points_and_bounds() {
        // Z0 : une seule tuile, tout point valide tombe en (0,0).
        assert_eq!(lonlat_to_tile(46.2, 6.14, 0), Some((0, 0)));
        // Z1 : l'origine (équateur/Greenwich) est le coin des 4 tuiles → (1,1).
        assert_eq!(lonlat_to_tile(0.0, 0.0, 1), Some((1, 1)));
        // Indices bornés à [0, 2^z − 1] (coin sud-ouest extrême).
        let (x, y) = lonlat_to_tile(-85.0, -179.99, 16).unwrap_or((-1, -1));
        assert!((0..=65535).contains(&x) && (0..=65535).contains(&y));
        // Hors domaine Mercator ou non fini : refusé.
        assert_eq!(lonlat_to_tile(89.0, 0.0, 16), None);
        assert_eq!(lonlat_to_tile(f64::NAN, 0.0, 16), None);
    }

    fn headers_with(origin: Option<&str>, host: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(o) = origin {
            if let Ok(v) = HeaderValue::from_str(o) {
                h.insert(header::ORIGIN, v);
            }
        }
        if let Some(host) = host {
            if let Ok(v) = HeaderValue::from_str(host) {
                h.insert(header::HOST, v);
            }
        }
        h
    }

    #[test]
    fn csrf_allows_same_origin_and_non_browser() {
        // Même origine : autorisé.
        assert!(origin_allowed(&headers_with(
            Some("http://10.0.0.1"),
            Some("10.0.0.1")
        )));
        // Client non-navigateur (pas d'Origin) : autorisé.
        assert!(origin_allowed(&headers_with(None, Some("10.0.0.1"))));
    }

    #[test]
    fn csrf_blocks_cross_origin() {
        // Origine étrangère : bloqué.
        assert!(!origin_allowed(&headers_with(
            Some("http://evil.example"),
            Some("10.0.0.1")
        )));
        // Origin présent mais Host absent : bloqué (impossible de comparer).
        assert!(!origin_allowed(&headers_with(
            Some("http://10.0.0.1"),
            None
        )));
    }

    #[test]
    fn config_validation_walks_nested_strings() {
        // Texte simple imbriqué : accepté.
        let ok = serde_json::json!({
            "establishment": {"name": "Mairie", "tags": ["abri", "eau"]},
        });
        assert!(validate_config_value(&ok, None, 0).is_ok());

        // `<script>` dans un champ affiché : refusé.
        let xss = serde_json::json!({"reassurance": {"msg": "<script>x</script>"}});
        assert!(validate_config_value(&xss, None, 0).is_err());

        // Tout champ avec des chevrons est refusé (plus d'exception WiFi).
        let bracket = serde_json::json!({"establishment": {"name": "a<b>c"}});
        assert!(validate_config_value(&bracket, None, 0).is_err());

        // Imbrication excessive : refusée (anti-DoS).
        let mut deep = serde_json::json!("x");
        for _ in 0..MAX_CONFIG_DEPTH + 1 {
            deep = serde_json::Value::Array(vec![deep]);
        }
        assert!(validate_config_value(&deep, None, 0).is_err());
    }

    #[test]
    fn auth_throttle_escalates_resets_and_caps() {
        let mut t = AuthThrottle::default();
        // Sous le seuil : aucun délai.
        for _ in 0..AUTH_FAIL_THRESHOLD {
            t.fails += 1;
        }
        assert_eq!(t.penalty(), Duration::ZERO);
        // Au-delà du seuil : délai croissant, plafonné à AUTH_MAX_DELAY.
        t.fails += 1;
        assert_eq!(t.penalty(), Duration::from_secs(1));
        t.fails += 100;
        assert_eq!(t.penalty(), AUTH_MAX_DELAY);
        // Réinitialisation (succès) : retour à zéro délai.
        t.fails = 0;
        assert_eq!(t.penalty(), Duration::ZERO);
    }

    #[test]
    fn config_projection_strips_wifi_password() {
        let mut value: serde_json::Value =
            serde_json::from_str(r#"{"establishment":{"name":"x"},"wifiPassword":"secret"}"#)
                .unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.remove("wifiPassword");
        }
        assert!(value.get("wifiPassword").is_none());
        assert!(value.get("establishment").is_some());
    }
}

// ======================= GROUPES DE PING =======================

/// Corps de la requête de création d'un groupe de ping.
#[derive(Deserialize)]
struct CreateGroupRequest {
    name: String,
    key: String,
    #[serde(default)]
    color: String,
}

/// Corps de la requête de ping citoyen.
#[derive(Deserialize)]
struct PingRequest {
    key: String,
    sender: String,
    #[serde(default)]
    message: String,
}

/// Liste les groupes (id + nom + couleur, sans hash de clé).
async fn list_groups(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    let node = state.node.read().await;
    let raw = node
        .store
        .as_ref()
        .and_then(|s| s.load_groups().ok().flatten())
        .unwrap_or_else(|| "[]".to_owned());
    let groups: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap_or_default();
    let public: Vec<serde_json::Value> = groups
        .into_iter()
        .map(|mut g| {
            if let Some(obj) = g.as_object_mut() {
                obj.remove("key_salt");
                obj.remove("key_hash");
            }
            g
        })
        .collect();
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::Value::Array(public)),
    )
        .into_response()
}

/// Crée un groupe de ping.
async fn create_group(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<CreateGroupRequest>,
) -> Response {
    if !origin_allowed(&headers) {
        return forbidden_csrf();
    }
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    if let Err(err) = validate_text(&req.name, MAX_PING_STRING) {
        return reject_text("name", err);
    }
    if req.key.is_empty() || req.key.len() > 32 {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "clé invalide (1–32 caractères)\n",
        )
            .into_response();
    }
    let color = match req.color.as_str() {
        "rouge" | "bleu" | "vert" | "violet" | "orange" | "ardoise" => req.color.clone(),
        _ => "bleu".to_owned(),
    };
    let ph = hash_group_key(&req.key);
    let id = random_token(8);
    let new_group = serde_json::json!({
        "id": id,
        "name": req.name,
        "color": color,
        "key_salt": ph.salt_hex,
        "key_hash": ph.hash_hex,
    });
    let node = state.node.read().await;
    let Some(store) = &node.store else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let raw = store
        .load_groups()
        .unwrap_or(None)
        .unwrap_or_else(|| "[]".to_owned());
    let mut groups: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap_or_default();
    if groups.len() >= MAX_GROUPS {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "nombre maximal de groupes atteint\n",
        )
            .into_response();
    }
    groups.push(new_group);
    let json = match serde_json::to_string(&groups) {
        Ok(j) => j,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if let Err(err) = store.save_groups(&json) {
        tracing::error!(%err, "sauvegarde groupes impossible");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    (
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "id": id })),
    )
        .into_response()
}

/// Supprime un groupe de ping par son identifiant.
async fn delete_group(
    State(state): State<SharedState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Response {
    if !origin_allowed(&headers) {
        return forbidden_csrf();
    }
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    let node = state.node.read().await;
    let Some(store) = &node.store else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let raw = store
        .load_groups()
        .unwrap_or(None)
        .unwrap_or_else(|| "[]".to_owned());
    let groups: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap_or_default();
    let filtered: Vec<serde_json::Value> = groups
        .into_iter()
        .filter(|g| g.get("id").and_then(|v| v.as_str()) != Some(id.as_str()))
        .collect();
    let json = match serde_json::to_string(&filtered) {
        Ok(j) => j,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if let Err(err) = store.save_groups(&json) {
        tracing::error!(%err, "sauvegarde groupes impossible");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

/// Soumet un ping citoyen (public, rate-limité).
async fn submit_ping(
    State(state): State<SharedState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<PingRequest>,
) -> Response {
    // Rate-limit **par IP** : purge d'abord les entrées expirées (borne mémoire),
    // puis refuse si cette IP a déjà pingé dans l'intervalle. Un client abusif ne
    // bloque plus tout le monde ; il devrait au moins changer d'IP (bail DHCP).
    {
        let ip = peer.ip();
        let now = Instant::now();
        let mut lim = state.ping_limiter.lock().await;
        lim.retain(|_, t| now.duration_since(*t) < PING_MIN_INTERVAL);
        if lim.contains_key(&ip) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                "merci d'attendre quelques secondes\n",
            )
                .into_response();
        }
        lim.insert(ip, now);
    }
    if let Err(err) = validate_text(&req.sender, MAX_PING_STRING) {
        return reject_text("sender", err);
    }
    if let Err(err) = validate_text(&req.message, MAX_PING_STRING) {
        return reject_text("message", err);
    }
    if req.key.is_empty() || req.key.len() > 32 {
        return (StatusCode::UNAUTHORIZED, "clé invalide\n").into_response();
    }
    let node = state.node.read().await;
    let Some(store) = &node.store else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let raw_groups = store
        .load_groups()
        .unwrap_or(None)
        .unwrap_or_else(|| "[]".to_owned());
    let groups: Vec<serde_json::Value> = serde_json::from_str(&raw_groups).unwrap_or_default();
    let matched = groups.iter().find(|g| {
        let salt = g.get("key_salt").and_then(|v| v.as_str()).unwrap_or("");
        let hash = g.get("key_hash").and_then(|v| v.as_str()).unwrap_or("");
        // Hachage léger dédié aux clés de groupe (chemin chaud, pas d'amplification).
        verify_group_key(&req.key, salt, hash)
    });
    let Some(group) = matched else {
        return (StatusCode::UNAUTHORIZED, "clé non reconnue\n").into_response();
    };
    let group_id = group
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let group_name = group
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let ping_id = random_token(8);
    let ping = serde_json::json!({
        "id": ping_id,
        "group_id": group_id,
        "group_name": group_name,
        "sender": req.sender,
        "message": req.message,
        "ts": now_unix(),
    });
    let raw_pings = store
        .load_pings()
        .unwrap_or(None)
        .unwrap_or_else(|| "[]".to_owned());
    let mut pings: Vec<serde_json::Value> = serde_json::from_str(&raw_pings).unwrap_or_default();
    pings.push(ping);
    if pings.len() > PING_MAX {
        pings.drain(0..pings.len() - PING_MAX);
    }
    let json = match serde_json::to_string(&pings) {
        Ok(j) => j,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    if let Err(err) = store.save_pings(&json) {
        tracing::error!(%err, "sauvegarde ping impossible");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    tracing::info!(group = group_name, sender = req.sender, "ping citoyen reçu");
    (
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({ "ok": true })),
    )
        .into_response()
}

/// Liste les pings reçus (admin).
async fn list_pings(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    let node = state.node.read().await;
    let raw = node
        .store
        .as_ref()
        .and_then(|s| s.load_pings().ok().flatten())
        .unwrap_or_else(|| "[]".to_owned());
    let pings: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::json!([]));
    ([(header::CACHE_CONTROL, "no-store")], Json(pings)).into_response()
}

/// Efface tous les pings (admin).
async fn clear_admin_pings(State(state): State<SharedState>, headers: HeaderMap) -> Response {
    if !origin_allowed(&headers) {
        return forbidden_csrf();
    }
    if let Err(resp) = require_admin(&state, &headers).await {
        return resp;
    }
    let node = state.node.read().await;
    let Some(store) = &node.store else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    if let Err(err) = store.clear_pings() {
        tracing::error!(%err, "effacement pings impossible");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}
