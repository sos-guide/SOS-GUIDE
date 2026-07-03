//! Politique d'émission sur LoRa — **règle non négociable**.
//!
//! LoRa est **d'abord le canal d'alerte** (ligne de vie). Le relais de paiement
//! est purement **best-effort** :
//! - il ne s'émet **jamais** tant qu'une alerte est active (le canal lui appartient) ;
//! - il est **rate-limité** (espacement minimal entre trames) pour respecter le
//!   duty cycle réglementaire et ne pas saturer le mesh.
//!
//! Fonctions **pures** (temps injecté) : aucune horloge cachée, entièrement testable.

/// Espacement minimal (secondes) entre deux trames de paiement. Volontairement
/// large : le paiement cède le canal, il ne le monopolise pas.
pub const MIN_SEND_INTERVAL_SECS: i64 = 10;

/// Décide si une trame de paiement peut être émise **maintenant**.
///
/// - `alert_active` : une alerte occupe le canal → paiement **refusé** ;
/// - `last_sent` : horodatage (epoch s) de la dernière trame de paiement émise,
///   ou `None` si aucune ;
/// - `now` : horodatage courant (epoch s).
#[must_use]
pub fn may_send_payment(alert_active: bool, last_sent: Option<i64>, now: i64) -> bool {
    if alert_active {
        return false;
    }
    match last_sent {
        Some(prev) => now.saturating_sub(prev) >= MIN_SEND_INTERVAL_SECS,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alert_always_preempts_payment() {
        // Même si l'espacement serait respecté, une alerte active bloque tout.
        assert!(!may_send_payment(true, None, 1_000));
        assert!(!may_send_payment(true, Some(0), 1_000));
    }

    #[test]
    fn respects_minimum_interval_when_idle() {
        // Première trame : autorisée.
        assert!(may_send_payment(false, None, 100));
        // Trop tôt après la précédente : refusée.
        assert!(!may_send_payment(false, Some(100), 105));
        // Après l'intervalle : autorisée.
        assert!(may_send_payment(false, Some(100), 100 + MIN_SEND_INTERVAL_SECS));
    }
}
