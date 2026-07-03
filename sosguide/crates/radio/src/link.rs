//! Abstraction du lien radio : une trame JSON par message, ligne par ligne.
//!
//! Le maillage LoRa transporte des trames texte ([`sos_core::AlertPacket::to_frame`]).
//! On masque le matériel derrière [`RadioLink`], qui se **scinde** en deux moitiés
//! ([`FrameSink`] clonable pour émettre, [`FrameSource`] pour recevoir) : ainsi
//! l'orchestrateur peut, dans un même `select!`, attendre une réception **et**
//! émettre sans conflit d'emprunt. Deux implémentations :
//! - [`SimLink`] : en mémoire (canaux Tokio), pour les tests et le mode `simulate` ;
//! - le pilote série réel (SX1276 / Meshtastic T-Beam) est **différé**
//!   (cf. [`crate::device`], gaté `live`, matériel absent pour l'instant).

use std::future::Future;

use tokio::sync::mpsc;

/// Moitié émettrice : clonable, émet via `&self` (plusieurs producteurs).
pub trait FrameSink: Clone + Send + Sync + 'static {
    /// Émet une trame sur le maillage.
    fn send(&self, frame: &str) -> impl Future<Output = std::io::Result<()>> + Send;
}

/// Moitié réceptrice : `recv` renvoie `None` quand le lien est fermé.
pub trait FrameSource: Send + 'static {
    /// Attend la prochaine trame reçue, `None` si le lien est fermé.
    fn recv(&mut self) -> impl Future<Output = Option<String>> + Send;
}

/// Lien radio scindable en une moitié émettrice et une moitié réceptrice.
pub trait RadioLink {
    /// Type de la moitié émettrice.
    type Tx: FrameSink;
    /// Type de la moitié réceptrice.
    type Rx: FrameSource;
    /// Scinde le lien en (émetteur, récepteur).
    fn split(self) -> (Self::Tx, Self::Rx);
}

/// Lien simulé en mémoire : les trames émises sont poussées dans un canal
/// observable, les trames reçues proviennent d'un canal d'injection. Sans
/// dépendance matérielle — sert aux tests et au mode `simulate`.
pub struct SimLink {
    sent: mpsc::UnboundedSender<String>,
    inbound: mpsc::UnboundedReceiver<String>,
    /// En mode `sink`, conserve l'émetteur entrant pour garder le canal ouvert
    /// (la réception reste en attente au lieu de renvoyer `None` immédiatement).
    keepalive: Option<mpsc::UnboundedSender<String>>,
}

impl SimLink {
    /// Crée un lien simulé. Retourne le lien, un récepteur observant les trames
    /// **émises**, et un émetteur permettant d'**injecter** des trames reçues.
    #[must_use]
    pub fn new() -> (
        Self,
        mpsc::UnboundedReceiver<String>,
        mpsc::UnboundedSender<String>,
    ) {
        let (sent_tx, sent_rx) = mpsc::unbounded_channel();
        let (in_tx, in_rx) = mpsc::unbounded_channel();
        (
            Self {
                sent: sent_tx,
                inbound: in_rx,
                keepalive: None,
            },
            sent_rx,
            in_tx,
        )
    }

    /// Crée un lien simulé « muet » : émet vers un puits jeté, ne reçoit jamais
    /// (la réception reste en attente). Pour le mode `simulate` où l'on observe
    /// les trames émises via les logs.
    #[must_use]
    pub fn sink() -> Self {
        let (sent_tx, _sent_rx) = mpsc::unbounded_channel();
        let (in_tx, in_rx) = mpsc::unbounded_channel();
        Self {
            sent: sent_tx,
            inbound: in_rx,
            keepalive: Some(in_tx), // garde le canal entrant ouvert
        }
    }
}

/// Moitié émettrice de [`SimLink`].
#[derive(Clone)]
pub struct SimTx {
    sent: mpsc::UnboundedSender<String>,
}

/// Moitié réceptrice de [`SimLink`].
pub struct SimRx {
    inbound: mpsc::UnboundedReceiver<String>,
    _keepalive: Option<mpsc::UnboundedSender<String>>,
}

impl RadioLink for SimLink {
    type Tx = SimTx;
    type Rx = SimRx;

    fn split(self) -> (Self::Tx, Self::Rx) {
        (
            SimTx { sent: self.sent },
            SimRx {
                inbound: self.inbound,
                _keepalive: self.keepalive,
            },
        )
    }
}

impl FrameSink for SimTx {
    async fn send(&self, frame: &str) -> std::io::Result<()> {
        // L'échec d'envoi (récepteur abandonné) n'est pas fatal en simulation.
        let _ = self.sent.send(frame.to_owned());
        Ok(())
    }
}

impl FrameSource for SimRx {
    async fn recv(&mut self) -> Option<String> {
        self.inbound.recv().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[tokio::test]
    async fn sent_frames_are_observable() -> TestResult {
        let (link, mut sent, _inject) = SimLink::new();
        let (tx, _rx) = link.split();
        tx.send("trame-1").await?;
        tx.send("trame-2").await?;
        assert_eq!(sent.recv().await, Some("trame-1".to_owned()));
        assert_eq!(sent.recv().await, Some("trame-2".to_owned()));
        Ok(())
    }

    #[tokio::test]
    async fn injected_frames_are_received() -> TestResult {
        let (link, _sent, inject) = SimLink::new();
        let (_tx, mut rx) = link.split();
        inject.send("entrante".to_owned())?;
        assert_eq!(rx.recv().await, Some("entrante".to_owned()));
        Ok(())
    }

    #[tokio::test]
    async fn recv_returns_none_when_closed() {
        let (link, _sent, inject) = SimLink::new();
        let (_tx, mut rx) = link.split();
        drop(inject);
        assert_eq!(rx.recv().await, None);
    }
}
