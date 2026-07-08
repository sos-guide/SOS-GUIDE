//! `sos-pay` — relais best-effort de **transactions Bitcoin signées** sur le mesh
//! LoRa (« Bitcoin tx over LoRa »), pour le **mode urgence** (WiFi local + LoRa,
//! sans Internet). Décision produit : voir `ASK.md` (2026-07-03).
//!
//! # Modèle — la borne est un TRANSPORTEUR, pas un portefeuille
//! Elle ne détient **ni clé ni fonds**. Un client signe sa transaction sur son
//! propre portefeuille et la remet à la borne (hex). La borne : la valide et la met
//! en file ([`queue`]), la fragmente en trames LoRa ([`frame`]) et la relaie
//! **best-effort** — les alertes priment **toujours** ([`policy`]). Un **nœud-sortie**
//! encore connecté à Internet la **diffuse** ([`broadcast`], `curl` → API publique).
//!
//! # Ce que ça NE fait PAS (limites actées)
//! Aucun réseau local ne **confirme** une transaction : la confirmation reste
//! l'affaire des mineurs mondiaux. En **îlot total** (aucune sortie Internet
//! joignable dans la portée mesh), la transaction reste simplement en attente. Le
//! risque de double-dépense hors-ligne est **assumé** (petits montants / clients
//! connus). Ce module vit **isolé** du portail vital et **désactivé par défaut**.
//!
//! # Modes (`SOS_PAY_MODE`, défaut `off`)
//! - `off` : rien n'est démarré — **aucun impact sur le portail vital** ;
//! - `simulate` : file + fragmentation en mémoire, sans matériel ni diffusion ;
//! - `live` : relais LoRa réel + diffusion — **différé** (aucun matériel LoRa branché).

pub mod broadcast;
pub mod frame;
pub mod policy;
pub mod queue;
pub mod tx;

use frame::Fragment;
use queue::{QueueError, TxQueue};
use tx::{PayTx, TxError, TxStatus};

/// Mode d'exécution du relais de paiement, dérivé de `SOS_PAY_MODE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PayMode {
    /// Ne rien démarrer (défaut sûr).
    #[default]
    Off,
    /// File + fragmentation en mémoire, sans matériel ni diffusion.
    Simulate,
    /// Relais LoRa + diffusion réels. **Différé** (aucun matériel LoRa).
    Live,
}

impl PayMode {
    /// Interprète une valeur d'environnement ; toute valeur inconnue (ou absente)
    /// retombe sur [`PayMode::Off`] — le défaut sûr.
    #[must_use]
    pub fn from_env_value(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "simulate" | "sim" => Self::Simulate,
            "live" => Self::Live,
            _ => Self::Off,
        }
    }
}

/// Configuration du relais de paiement.
#[derive(Debug, Clone)]
pub struct PayConfig {
    /// Mode d'exécution.
    pub mode: PayMode,
    /// URL de diffusion utilisée par le nœud-sortie (API publique).
    pub broadcast_api: String,
}

impl Default for PayConfig {
    fn default() -> Self {
        Self {
            mode: PayMode::Off,
            broadcast_api: broadcast::DEFAULT_BROADCAST_API.to_owned(),
        }
    }
}

/// Erreur du relais.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PayError {
    /// Transaction invalide (format/taille).
    #[error(transparent)]
    Tx(#[from] TxError),
    /// Mise en file impossible (pleine ou doublon).
    #[error(transparent)]
    Queue(#[from] QueueError),
    /// Identifiant de transaction inconnu de la file.
    #[error("transaction inconnue")]
    Unknown,
}

/// Relais de paiement : compose la file, la fragmentation et la diffusion.
///
/// Surface synchrone et **pure d'effets de bord** (aucune E/S) : elle prépare tout
/// (file, fragments, arguments de diffusion). L'émission LoRa et l'exécution de
/// `curl` relèvent de l'orchestrateur `live` (différé) qui consommera ces sorties.
pub struct Relay {
    config: PayConfig,
    queue: TxQueue,
}

impl Relay {
    /// Nouveau relais pour la configuration donnée.
    #[must_use]
    pub fn new(config: PayConfig) -> Self {
        Self {
            config,
            queue: TxQueue::new(),
        }
    }

    /// Configuration active.
    #[must_use]
    pub fn config(&self) -> &PayConfig {
        &self.config
    }

    /// File courante (lecture seule).
    #[must_use]
    pub fn queue(&self) -> &TxQueue {
        &self.queue
    }

    /// Accepte une transaction signée (hex), la valide et la met en file. Renvoie
    /// son identifiant local. Aucune clé ni fonds n'est manipulé.
    pub fn accept_hex(&mut self, hex: &str) -> Result<String, PayError> {
        let tx = PayTx::parse_hex(hex)?;
        let id = tx.id().to_owned();
        self.queue.enqueue(tx)?;
        Ok(id)
    }

    /// Accepte une transaction **réassemblée depuis le maillage** (octets bruts),
    /// la valide et la met en file. Renvoie son identifiant local. Un doublon
    /// (déjà reçue) n'est pas une erreur bloquante côté appelant.
    pub fn accept_raw(&mut self, raw: Vec<u8>) -> Result<String, PayError> {
        let tx = PayTx::from_raw(raw)?;
        let id = tx.id().to_owned();
        self.queue.enqueue(tx)?;
        Ok(id)
    }

    /// Fragmente une transaction en file pour l'émission LoRa et la marque
    /// [`TxStatus::Relayed`]. Renvoie ses fragments.
    pub fn relay_fragments(&mut self, id: &str) -> Result<Vec<Fragment>, PayError> {
        let tx = self
            .queue
            .all()
            .iter()
            .find(|t| t.id() == id)
            .ok_or(PayError::Unknown)?;
        let frags = frame::fragment(tx);
        self.queue.mark(id, TxStatus::Relayed);
        Ok(frags)
    }

    /// Marque une transaction diffusée et construit les arguments `curl` que le
    /// nœud-sortie exécutera pour la pousser vers l'API publique.
    pub fn broadcast_command(&mut self, id: &str) -> Result<Vec<String>, PayError> {
        let hex = self
            .queue
            .all()
            .iter()
            .find(|t| t.id() == id)
            .map(PayTx::hex)
            .ok_or(PayError::Unknown)?;
        self.queue.mark(id, TxStatus::Broadcast);
        Ok(broadcast::broadcast_argv(&self.config.broadcast_api, &hex))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frame::Reassembler;

    type R = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn mode_defaults_off_and_parses_known_values() {
        assert_eq!(PayMode::from_env_value("simulate"), PayMode::Simulate);
        assert_eq!(PayMode::from_env_value(" LIVE "), PayMode::Live);
        assert_eq!(PayMode::from_env_value("nimportequoi"), PayMode::Off);
        assert_eq!(PayMode::from_env_value(""), PayMode::Off);
        assert_eq!(PayConfig::default().mode, PayMode::Off);
    }

    #[test]
    fn full_pipeline_accept_fragment_reassemble_broadcast() -> R {
        // Une « transaction » de 300 octets (contenu arbitraire mais valide en format).
        let raw_hex = "ab".repeat(300);
        let mut relay = Relay::new(PayConfig {
            mode: PayMode::Simulate,
            ..PayConfig::default()
        });

        // 1) La borne accepte la tx signée du client.
        let id = relay.accept_hex(&raw_hex)?;
        assert_eq!(relay.queue().len(), 1);

        // 2) Fragmentation pour le mesh LoRa (marque « Relayed »).
        let frags = relay.relay_fragments(&id)?;
        assert_eq!(frags.len(), 3);

        // 3) Un nœud voisin réassemble les fragments reçus (dans le désordre).
        let mut re = Reassembler::new();
        let mut reassembled = None;
        for idx in [1usize, 2, 0] {
            let f = frags.get(idx).ok_or("fragment manquant")?;
            if let Some(raw) = re.ingest(f)? {
                reassembled = Some(raw);
            }
        }
        // La tx reconstruite est identique à l'originale.
        assert_eq!(
            reassembled.map(|r| tx::hex_encode(&r)),
            Some(raw_hex.clone())
        );

        // 4) Le nœud-sortie diffuse : commande curl vers l'API publique.
        let argv = relay.broadcast_command(&id)?;
        assert_eq!(
            argv.last().map(String::as_str),
            Some("https://mempool.space/api/tx")
        );
        assert!(argv.contains(&raw_hex));

        // La file n'a plus rien en attente (tx diffusée).
        assert_eq!(relay.queue().pending().count(), 0);
        Ok(())
    }

    #[test]
    fn unknown_id_is_rejected() {
        let mut relay = Relay::new(PayConfig::default());
        assert_eq!(relay.relay_fragments("inconnu"), Err(PayError::Unknown));
        assert_eq!(relay.broadcast_command("inconnu"), Err(PayError::Unknown));
    }
}
