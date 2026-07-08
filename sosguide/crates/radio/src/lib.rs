//! Liaison radio LoRa du maillage : transport des trames d'alerte signées,
//! relais multi-sauts, déduplication et anti-rejeu.
//!
//! **Priorité au vital : les alertes (`AlertPacket`) priment TOUJOURS.** Le canal
//! transporte aussi, **en best-effort et alertes-first**, des **fragments de
//! transaction Bitcoin signée** ([`sos_pay`], « Bitcoin tx over LoRa », mode
//! urgence) — uniquement si le relais de paiement est activé (sinon ces trames
//! sont ignorées). Le codec de trame d'alerte vit dans [`sos_core`], la crypto dans
//! [`sos_security`], le codec de fragments dans [`sos_pay::frame`]. Ici : le
//! **transport** ([`link`]), la **décision de relais** pure ([`relay`]) et
//! l'**orchestrateur** (sélection biaisée : réception + alertes avant paiement).
//!
//! # Modes ([`RadioMode`], via `SOS_RADIO_MODE`)
//!
//! - **`off`** (défaut) : aucune tâche, aucun périphérique ouvert.
//! - **`simulate`** : transport en mémoire ([`link::SimLink`]) — les trames émises
//!   sont journalisées, aucune émission radio réelle. Pour le test hors matériel.
//! - **`live`** : pilote série/SPI réel ([`device`]). **Différé** : aucun matériel
//!   LoRa n'est branché sur le Pi de dev ; `device::open` échoue proprement et
//!   l'orchestrateur retombe sur un no-op journalisé.

pub mod device;
pub mod link;
pub mod relay;

use std::sync::Arc;

use sos_core::AlertInbox;
use sos_pay::frame::{decode_frame, Reassembler};
use sos_pay::Relay;
use sos_security::KeyRing;
use tokio::sync::{mpsc, Mutex, RwLock};

use crate::link::{FrameSink, FrameSource, RadioLink, SimLink};
use crate::relay::{evaluate, ReceiveOutcome, DEFAULT_MAX_HOP};

/// Canaux de paiement passés à l'orchestrateur (émission des fragments + puits de
/// réassemblage). Regroupés pour garder les signatures lisibles.
pub struct PayChannels {
    /// Fragments de transaction à **émettre** (best-effort, alertes-first).
    pub outgoing: mpsc::Receiver<String>,
    /// Relais partagé où **déposer** les transactions réassemblées reçues du
    /// maillage. `None` ⇒ paiement désactivé : les fragments entrants sont ignorés.
    pub relay: Option<Arc<Mutex<Relay>>>,
}

/// Mode d'exécution de la radio, dérivé de `SOS_RADIO_MODE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RadioMode {
    /// Ne rien démarrer (défaut sûr).
    #[default]
    Off,
    /// Transport en mémoire, sans matériel.
    Simulate,
    /// Pilote série/SPI réel (différé, matériel absent).
    Live,
}

impl RadioMode {
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

/// Configuration de la radio.
#[derive(Debug, Clone)]
pub struct RadioConfig {
    /// Mode d'exécution.
    pub mode: RadioMode,
    /// Chemin du périphérique série/SPI (mode `live`).
    pub device: String,
    /// Plafond de rebonds mesh.
    pub max_hop: u8,
}

impl Default for RadioConfig {
    fn default() -> Self {
        Self {
            mode: RadioMode::Off,
            device: "/dev/ttyUSB0".to_owned(),
            max_hop: DEFAULT_MAX_HOP,
        }
    }
}

/// Erreur du sous-système radio.
#[derive(Debug, thiserror::Error)]
pub enum RadioError {
    /// Le périphérique radio n'est pas disponible (pilote différé / non branché).
    #[error("périphérique radio indisponible : {0}")]
    DeviceUnavailable(String),
}

/// Horodatage Unix courant (secondes). Retombe sur 0 si l'horloge est antérieure
/// à l'époque — jamais de panique.
fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Lance l'orchestrateur radio.
///
/// `keyring` (partagé avec le portail) vérifie les signatures entrantes ;
/// `inbox` (partagée avec le portail) reçoit les alertes admises pour affichage ;
/// `local_frames` apporte les trames publiées localement à diffuser sur le mesh.
///
/// Retourne en mode `off`, ou quand le canal des trames locales et le lien sont
/// tous deux fermés.
pub async fn run(
    cfg: RadioConfig,
    keyring: Arc<RwLock<KeyRing>>,
    inbox: Arc<Mutex<AlertInbox>>,
    local_frames: mpsc::Receiver<String>,
    pay: PayChannels,
) -> Result<(), RadioError> {
    match cfg.mode {
        RadioMode::Off => {
            tracing::info!("radio: mode off — aucune tâche démarrée");
            Ok(())
        }
        RadioMode::Simulate => {
            tracing::info!(max_hop = cfg.max_hop, "radio: démarrage en simulation");
            run_with(SimLink::sink(), cfg, keyring, inbox, local_frames, pay).await;
            Ok(())
        }
        RadioMode::Live => {
            // Pilote matériel différé : échoue proprement, pas de panique.
            match device::open(&cfg.device).await {
                Ok(()) => {
                    // Quand le vrai pilote existera, il fournira un RadioLink ici.
                    tracing::warn!("radio: pilote live ouvert mais non câblé — no-op");
                    Ok(())
                }
                Err(err) => {
                    tracing::warn!(%err, "radio: live indisponible — aucune émission");
                    Err(err)
                }
            }
        }
    }
}

/// Boucle de service générique sur un lien. **Sélection biaisée** (`biased`) :
/// (1) réception mesh, (2) alerte locale, (3) fragment de paiement — les alertes
/// passent donc **toujours avant** le paiement. Testable avec [`SimLink`].
async fn run_with<L: RadioLink>(
    link: L,
    cfg: RadioConfig,
    keyring: Arc<RwLock<KeyRing>>,
    inbox: Arc<Mutex<AlertInbox>>,
    mut local_frames: mpsc::Receiver<String>,
    mut pay: PayChannels,
) {
    let (tx, mut rx) = link.split();
    let mut reasm = Reassembler::new();
    let mut pay_open = true;
    loop {
        tokio::select! {
            biased;
            // 1) Trame entrante du maillage : alerte (vérif/dédup/relais) ou, à
            //    défaut, fragment de paiement (réassemblage).
            incoming = rx.recv() => {
                let Some(raw) = incoming else {
                    tracing::info!("radio: lien fermé — fin de l'orchestrateur");
                    return;
                };
                handle_incoming(&raw, &tx, &keyring, &inbox, &mut reasm, &pay.relay, cfg.max_hop).await;
            }
            // 2) Alerte publiée localement : priorité vitale, diffusée aussitôt.
            local = local_frames.recv() => {
                match local {
                    Some(frame) => {
                        if let Err(err) = tx.send(&frame).await {
                            tracing::warn!(%err, "radio: émission de la trame locale impossible");
                        } else {
                            tracing::info!("radio: trame locale diffusée sur le maillage");
                        }
                    }
                    None => {
                        tracing::info!("radio: canal local fermé — fin de l'orchestrateur");
                        return;
                    }
                }
            }
            // 3) Fragment de paiement à émettre : **best-effort, après les alertes**.
            frag = pay.outgoing.recv(), if pay_open => {
                match frag {
                    Some(frame) => {
                        if let Err(err) = tx.send(&frame).await {
                            tracing::warn!(%err, "radio: émission d'un fragment de paiement impossible");
                        } else {
                            tracing::trace!("radio: fragment de paiement émis (best-effort)");
                        }
                    }
                    // Canal de paiement fermé : on cesse de le sonder (aucun arrêt).
                    None => pay_open = false,
                }
            }
        }
    }
}

/// Traite une trame entrante : d'abord comme **alerte** (vérif/dédup/relais) ;
/// si elle n'en est pas une (`Malformed`), tente un **fragment de paiement**.
async fn handle_incoming<T: FrameSink>(
    raw: &str,
    tx: &T,
    keyring: &Arc<RwLock<KeyRing>>,
    inbox: &Arc<Mutex<AlertInbox>>,
    reasm: &mut Reassembler,
    pay_relay: &Option<Arc<Mutex<Relay>>>,
    max_hop: u8,
) {
    let outcome = {
        let kr = keyring.read().await;
        let mut ib = inbox.lock().await;
        evaluate(raw, &kr, &mut ib, now_unix(), max_hop)
    };
    match outcome {
        ReceiveOutcome::Admitted { relay } => {
            tracing::info!("radio: alerte mesh admise");
            if let Some(fwd) = relay {
                if let Err(err) = tx.send(&fwd).await {
                    tracing::warn!(%err, "radio: relais de la trame impossible");
                }
            }
        }
        ReceiveOutcome::Duplicate => tracing::trace!("radio: trame dupliquée ignorée"),
        ReceiveOutcome::TooOld => tracing::trace!("radio: trame périmée ignorée"),
        ReceiveOutcome::Untrusted => tracing::warn!("radio: trame non signée/non fiable rejetée"),
        // Pas une alerte : peut-être un fragment de paiement (si le relais est actif).
        ReceiveOutcome::Malformed => handle_payment_fragment(raw, reasm, pay_relay).await,
    }
}

/// Réassemble un fragment de paiement ; à transaction complète, la met en file
/// dans le relais partagé. No-op si le paiement est désactivé (`pay_relay` = `None`).
async fn handle_payment_fragment(
    raw: &str,
    reasm: &mut Reassembler,
    pay_relay: &Option<Arc<Mutex<Relay>>>,
) {
    let Some(relay) = pay_relay else {
        return;
    };
    let Ok(frag) = decode_frame(raw) else {
        tracing::trace!("radio: trame illisible ignorée");
        return;
    };
    match reasm.ingest(&frag) {
        Ok(Some(raw_tx)) => {
            let mut r = relay.lock().await;
            match r.accept_raw(raw_tx) {
                Ok(id) => tracing::info!(%id, "radio: transaction reçue du mesh, mise en file"),
                Err(err) => tracing::warn!(%err, "radio: transaction du mesh rejetée"),
            }
        }
        Ok(None) => tracing::trace!("radio: fragment de paiement reçu (incomplet)"),
        Err(err) => tracing::trace!(%err, "radio: fragment de paiement invalide"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sos_core::{AlertPacket, AlertType};
    use sos_pay::frame::{encode_frame, fragment};
    use sos_pay::tx::PayTx;
    use sos_pay::{PayConfig, PayMode, Relay};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Canaux de paiement « inertes » : aucune émission, paiement désactivé.
    fn no_pay() -> PayChannels {
        let (_tx, rx) = mpsc::channel(1);
        PayChannels {
            outgoing: rx,
            relay: None,
        }
    }

    #[test]
    fn mode_parses_and_defaults_off() {
        assert_eq!(RadioMode::from_env_value("simulate"), RadioMode::Simulate);
        assert_eq!(RadioMode::from_env_value(" LIVE "), RadioMode::Live);
        assert_eq!(RadioMode::from_env_value("zzz"), RadioMode::Off);
        assert_eq!(RadioMode::from_env_value(""), RadioMode::Off);
    }

    #[tokio::test]
    async fn off_mode_returns_immediately() -> TestResult {
        let (_tx, rx) = mpsc::channel(1);
        let kr = Arc::new(RwLock::new(KeyRing::generate("n")));
        let inbox = Arc::new(Mutex::new(AlertInbox::new()));
        run(RadioConfig::default(), kr, inbox, rx, no_pay()).await?;
        Ok(())
    }

    #[tokio::test]
    async fn local_frame_is_broadcast_then_link_closes() -> TestResult {
        let (link, mut sent, _inject) = SimLink::new();
        let (tx, rx) = mpsc::channel(4);
        let kr = Arc::new(RwLock::new(KeyRing::generate("n")));
        let inbox = Arc::new(Mutex::new(AlertInbox::new()));
        let cfg = RadioConfig {
            mode: RadioMode::Simulate,
            ..RadioConfig::default()
        };
        tx.send("ma-trame".to_owned()).await?;
        drop(tx); // ferme le canal local → l'orchestrateur termine
        run_with(link, cfg, kr, inbox, rx, no_pay()).await;
        assert_eq!(sent.recv().await, Some("ma-trame".to_owned()));
        Ok(())
    }

    #[tokio::test]
    async fn incoming_trusted_alert_is_admitted_and_relayed() -> TestResult {
        // Source de confiance + récepteur partageant l'inbox.
        let sender = KeyRing::generate("src");
        let mut receiver = KeyRing::generate("rcv");
        let pubkey = sender.public_key_pem()?;
        receiver.load_trusted_nodes(
            &serde_json::json!({"nodes":{"src":{"public_key":pubkey}}}).to_string(),
        )?;

        let mut packet = AlertPacket::new("src", AlertType::Incendie, "feu", now_unix());
        packet.signature = Some(sender.sign(&packet));
        let frame = packet.to_frame()?;

        let (link, mut sent, inject) = SimLink::new();
        let (tx, rx) = mpsc::channel(1);
        let kr = Arc::new(RwLock::new(receiver));
        let inbox = Arc::new(Mutex::new(AlertInbox::new()));
        let cfg = RadioConfig {
            mode: RadioMode::Simulate,
            ..RadioConfig::default()
        };

        inject.send(frame)?; // injecte la trame entrante
        let inbox_probe = Arc::clone(&inbox);
        let handle =
            tokio::spawn(async move { run_with(link, cfg, kr, inbox, rx, no_pay()).await });
        // La trame relayée (hop=1) doit apparaître côté émission.
        let relayed = sent.recv().await.ok_or("aucune trame relayée")?;
        assert_eq!(AlertPacket::from_frame(&relayed)?.hop, 1);
        drop(tx); // ferme le canal local → l'orchestrateur termine proprement
        let _ = handle.await;
        assert_eq!(inbox_probe.lock().await.alerts().len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn incoming_payment_fragments_reassemble_into_relay() -> TestResult {
        // Un relais partagé (paiement activé) où la tx réassemblée doit atterrir.
        let relay = Arc::new(Mutex::new(Relay::new(PayConfig {
            mode: PayMode::Simulate,
            ..PayConfig::default()
        })));

        // Une « transaction » de 200 octets → 2 fragments.
        let tx_obj = PayTx::parse_hex(&"ab".repeat(200))?;
        let frags = fragment(&tx_obj);
        assert_eq!(frags.len(), 2);

        let (link, _sent, inject) = SimLink::new();
        for f in &frags {
            inject.send(encode_frame(f)?)?; // injecte les fragments entrants
        }
        // Canal d'alerte fermé → l'orchestrateur termine après avoir traité l'entrant.
        let (ltx, lrx) = mpsc::channel::<String>(1);
        drop(ltx);
        let cfg = RadioConfig {
            mode: RadioMode::Simulate,
            ..RadioConfig::default()
        };
        let kr = Arc::new(RwLock::new(KeyRing::generate("rcv")));
        let inbox = Arc::new(Mutex::new(AlertInbox::new()));
        let (_ptx, prx) = mpsc::channel(1);
        run_with(
            link,
            cfg,
            kr,
            inbox,
            lrx,
            PayChannels {
                outgoing: prx,
                relay: Some(Arc::clone(&relay)),
            },
        )
        .await;

        // La transaction complète est en file dans le relais.
        let guard = relay.lock().await;
        assert_eq!(guard.queue().len(), 1);
        assert_eq!(
            guard.queue().all().first().map(|t| t.id().to_owned()),
            Some(tx_obj.id().to_owned())
        );
        Ok(())
    }

    #[tokio::test]
    async fn outgoing_payment_fragment_is_emitted_best_effort() -> TestResult {
        let (link, mut sent, _inject) = SimLink::new();
        let cfg = RadioConfig {
            mode: RadioMode::Simulate,
            ..RadioConfig::default()
        };
        let kr = Arc::new(RwLock::new(KeyRing::generate("n")));
        let inbox = Arc::new(Mutex::new(AlertInbox::new()));

        // Canal d'alerte OUVERT mais inactif (pour que la boucle atteigne le paiement).
        let (ltx, lrx) = mpsc::channel::<String>(1);
        let (ptx, prx) = mpsc::channel(4);
        ptx.send("fragment-paiement".to_owned()).await?;
        let handle = tokio::spawn(async move {
            run_with(
                link,
                cfg,
                kr,
                inbox,
                lrx,
                PayChannels {
                    outgoing: prx,
                    relay: None,
                },
            )
            .await;
        });
        assert_eq!(sent.recv().await, Some("fragment-paiement".to_owned()));
        drop(ltx); // ferme l'alerte → fin propre
        let _ = handle.await;
        Ok(())
    }
}
