//! Alerte active du nœud : la consigne d'urgence actuellement diffusée.
//!
//! Une seule alerte active à la fois. Elle associe une **cause** typée
//! ([`AlertType`]) et des **consignes locales** (texte libre, propres au lieu).
//! Lever une alerte de cause [`AlertType::FinAlerte`] revient à clore : il n'y
//! a alors plus d'alerte active.
//!
//! Type purement domaine (sérialisable pour la persistance), sans dépendance
//! d'infrastructure. L'origine — déclenchement manuel par l'admin **ou**
//! ingestion automatique d'une source officielle (Phases ultérieures) — est
//! indifférente ici : les deux produisent la même [`ActiveAlert`].

use serde::{Deserialize, Serialize};

use crate::alert::{truncate_chars, AlertType};

/// Longueur maximale des consignes locales affichées sur la page SOS.
///
/// Bien plus large que [`crate::alert::MAX_MESSAGE_CHARS`] (limite de la trame
/// LoRa) : la page SOS locale n'a pas la contrainte des 255 octets du mesh. La
/// propagation mesh (Phases 3-4) tronquera au besoin lors de la mise en trame.
pub const MAX_INSTRUCTIONS_CHARS: usize = 2000;

/// Alerte actuellement active sur le nœud.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveAlert {
    /// Cause de l'alerte (type figé, interop v2.5).
    pub cause: AlertType,
    /// Consignes locales précises (texte libre, propre au lieu).
    pub instructions: String,
    /// Horodatage Unix du déclenchement.
    pub since: i64,
}

impl ActiveAlert {
    /// Construit une alerte active en bornant les consignes à
    /// [`MAX_INSTRUCTIONS_CHARS`] caractères.
    #[must_use]
    pub fn new(cause: AlertType, instructions: &str, since: i64) -> Self {
        Self {
            cause,
            instructions: truncate_chars(instructions, MAX_INSTRUCTIONS_CHARS).to_owned(),
            since,
        }
    }

    /// `true` si la cause signifie la fin de l'alerte (retour à la normale).
    #[must_use]
    pub fn is_end(&self) -> bool {
        matches!(self.cause, AlertType::FinAlerte)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_truncates_instructions_to_max() {
        let long: String = "é".repeat(MAX_INSTRUCTIONS_CHARS + 100);
        let alert = ActiveAlert::new(AlertType::Incendie, &long, 0);
        assert_eq!(alert.instructions.chars().count(), MAX_INSTRUCTIONS_CHARS);
    }

    #[test]
    fn is_end_only_for_fin_alerte() {
        assert!(ActiveAlert::new(AlertType::FinAlerte, "", 0).is_end());
        assert!(!ActiveAlert::new(AlertType::Incendie, "feu", 0).is_end());
    }

    #[test]
    fn serde_round_trip() -> Result<(), serde_json::Error> {
        let alert = ActiveAlert::new(AlertType::Evacuation, "Évacuez par l'est", 1_750_000_000);
        let json = serde_json::to_string(&alert)?;
        // La cause circule sous son nom de fil (interop v2.5).
        assert!(json.contains("\"EVACUATION\""));
        assert_eq!(serde_json::from_str::<ActiveAlert>(&json)?, alert);
        Ok(())
    }

    /// Garantie **vie privée** (exigence forte Phase 2.5) : une alerte ne porte
    /// QUE cause/consignes/horodatage — jamais d'identité de citoyen, de client
    /// connecté ou de donnée personnelle. Ce test fige le contrat : tout champ
    /// ajouté au modèle devra être revu sous l'angle de la confidentialité.
    #[test]
    fn active_alert_carries_no_personal_data() -> Result<(), serde_json::Error> {
        let alert = ActiveAlert::new(AlertType::Incendie, "Évacuez", 1);
        let value: serde_json::Value = serde_json::to_value(&alert)?;
        let keys: std::collections::BTreeSet<&str> = value
            .as_object()
            .map(|o| o.keys().map(String::as_str).collect())
            .unwrap_or_default();
        assert_eq!(
            keys,
            ["cause", "instructions", "since"].into_iter().collect()
        );
        Ok(())
    }
}
