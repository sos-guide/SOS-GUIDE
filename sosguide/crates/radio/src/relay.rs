//! Décision de relais mesh, **pure** et testable.
//!
//! À la réception d'une trame LoRa, le nœud :
//! 1. la décode ([`AlertPacket::from_frame`]) — sinon `Malformed` ;
//! 2. **vérifie la signature** contre le registre de confiance — une trame non
//!    signée ou d'un nœud inconnu est **jetée** (`Untrusted`) : jamais affichée,
//!    jamais relayée (une consigne d'urgence usurpée est pire que pas de consigne) ;
//! 3. l'admet dans la boîte de réception (dédup + anti-rejeu d'âge) ;
//! 4. si admise et sous le plafond de rebonds, prépare la **trame à rediffuser**
//!    (`hop` incrémenté) pour propager l'alerte au reste du maillage.
//!
//! Aucune E/S ici : l'horloge et les effets réseau sont fournis par l'appelant.

use sos_core::{Admission, AlertInbox, AlertPacket};
use sos_security::KeyRing;

/// Plafond de rebonds par défaut : au-delà, la trame n'est plus relayée
/// (borne la propagation, évite les tempêtes de diffusion dans le maillage).
pub const DEFAULT_MAX_HOP: u8 = 8;

/// Verdict du traitement d'une trame reçue du maillage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiveOutcome {
    /// Trame valide et admise. `relay` = trame à rediffuser (`hop` incrémenté)
    /// si le plafond n'est pas atteint, sinon `None`.
    Admitted {
        /// Trame à rediffuser au maillage, ou `None` si plafond de rebonds atteint.
        relay: Option<String>,
    },
    /// Déjà reçue (même source + même horodatage) : ignorée.
    Duplicate,
    /// Trop ancienne (anti-rejeu) : ignorée.
    TooOld,
    /// Signature absente, invalide, ou nœud non digne de confiance : **jetée**.
    Untrusted,
    /// Trame illisible (JSON/version/type invalides) : **jetée**.
    Malformed,
}

/// Traite une trame reçue : décode, vérifie, admet, et décide du relais.
///
/// `now` = horodatage Unix courant (anti-rejeu) ; `max_hop` = plafond de rebonds.
pub fn evaluate(
    raw: &str,
    keyring: &KeyRing,
    inbox: &mut AlertInbox,
    now: i64,
    max_hop: u8,
) -> ReceiveOutcome {
    let packet = match AlertPacket::from_frame(raw) {
        Ok(packet) => packet,
        Err(_) => return ReceiveOutcome::Malformed,
    };
    // Anti-usurpation : seule une signature reconnue est acceptée.
    if keyring.verify(&packet).is_err() {
        return ReceiveOutcome::Untrusted;
    }
    match inbox.admit(&packet, true, now) {
        Admission::Duplicate => ReceiveOutcome::Duplicate,
        Admission::TooOld => ReceiveOutcome::TooOld,
        Admission::Accepted => ReceiveOutcome::Admitted {
            relay: relay_frame(&packet, max_hop),
        },
    }
}

/// Construit la trame à rediffuser : copie du paquet avec `hop + 1`, tant que le
/// plafond n'est pas atteint. `None` si plafond atteint ou sérialisation impossible.
fn relay_frame(packet: &AlertPacket, max_hop: u8) -> Option<String> {
    if packet.hop >= max_hop {
        return None;
    }
    let mut forwarded = packet.clone();
    forwarded.hop = packet.hop.saturating_add(1);
    forwarded.to_frame().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sos_core::{AlertError, AlertType};

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Trousseau émetteur + récepteur se faisant mutuellement confiance.
    fn trusted_pair() -> Result<(KeyRing, KeyRing), Box<dyn std::error::Error>> {
        let sender = KeyRing::generate("nœud-source");
        let mut receiver = KeyRing::generate("nœud-récepteur");
        // Le récepteur charge la clé publique de la source comme nœud de confiance.
        let pubkey = sender.public_key_pem()?;
        let trust = serde_json::json!({
            "nodes": { "nœud-source": { "public_key": pubkey } }
        })
        .to_string();
        receiver.load_trusted_nodes(&trust)?;
        Ok((sender, receiver))
    }

    fn signed_frame(sender: &KeyRing, ts: i64, hop: u8) -> Result<String, AlertError> {
        let mut packet = AlertPacket::new(sender.node_id(), AlertType::Incendie, "feu", ts);
        packet.hop = hop;
        packet.signature = Some(sender.sign(&packet));
        packet.to_frame()
    }

    #[test]
    fn admits_and_relays_with_incremented_hop() -> TestResult {
        let (sender, receiver) = trusted_pair()?;
        let mut inbox = AlertInbox::new();
        let frame = signed_frame(&sender, 1_000, 0)?;
        let ReceiveOutcome::Admitted { relay: Some(fwd) } =
            evaluate(&frame, &receiver, &mut inbox, 1_010, DEFAULT_MAX_HOP)
        else {
            return Err("attendu Admitted avec relais".into());
        };
        assert_eq!(AlertPacket::from_frame(&fwd)?.hop, 1);
        assert_eq!(inbox.alerts().len(), 1);
        Ok(())
    }

    #[test]
    fn duplicate_is_not_readmitted() -> TestResult {
        let (sender, receiver) = trusted_pair()?;
        let mut inbox = AlertInbox::new();
        let frame = signed_frame(&sender, 2_000, 0)?;
        assert!(matches!(
            evaluate(&frame, &receiver, &mut inbox, 2_005, DEFAULT_MAX_HOP),
            ReceiveOutcome::Admitted { .. }
        ));
        assert_eq!(
            evaluate(&frame, &receiver, &mut inbox, 2_006, DEFAULT_MAX_HOP),
            ReceiveOutcome::Duplicate
        );
        Ok(())
    }

    #[test]
    fn too_old_is_rejected() -> TestResult {
        let (sender, receiver) = trusted_pair()?;
        let mut inbox = AlertInbox::new();
        let frame = signed_frame(&sender, 0, 0)?;
        assert_eq!(
            evaluate(&frame, &receiver, &mut inbox, 10_000, DEFAULT_MAX_HOP),
            ReceiveOutcome::TooOld
        );
        Ok(())
    }

    #[test]
    fn unsigned_or_unknown_node_is_untrusted() -> TestResult {
        let (_sender, receiver) = trusted_pair()?;
        let mut inbox = AlertInbox::new();
        // Paquet d'un nœud inconnu, signé par lui-même (non digne de confiance ici).
        let stranger = KeyRing::generate("inconnu");
        let frame = signed_frame(&stranger, 3_000, 0)?;
        assert_eq!(
            evaluate(&frame, &receiver, &mut inbox, 3_010, DEFAULT_MAX_HOP),
            ReceiveOutcome::Untrusted
        );
        assert!(inbox.alerts().is_empty());
        Ok(())
    }

    #[test]
    fn malformed_frame_is_dropped() -> TestResult {
        let (_s, receiver) = trusted_pair()?;
        let mut inbox = AlertInbox::new();
        assert_eq!(
            evaluate("pas du json", &receiver, &mut inbox, 0, DEFAULT_MAX_HOP),
            ReceiveOutcome::Malformed
        );
        Ok(())
    }

    #[test]
    fn hop_ceiling_admits_without_relay() -> TestResult {
        let (sender, receiver) = trusted_pair()?;
        let mut inbox = AlertInbox::new();
        // Trame déjà au plafond : admise localement mais non rediffusée.
        let frame = signed_frame(&sender, 4_000, DEFAULT_MAX_HOP)?;
        assert_eq!(
            evaluate(&frame, &receiver, &mut inbox, 4_010, DEFAULT_MAX_HOP),
            ReceiveOutcome::Admitted { relay: None }
        );
        Ok(())
    }
}
