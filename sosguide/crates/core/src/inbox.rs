//! Boîte de réception des alertes mesh : déduplication, anti-rejeu,
//! et projection prête à afficher pour le portail.
//!
//! Logique pure (aucune E/S, aucune horloge implicite) : l'horodatage
//! courant est toujours passé en paramètre, ce qui rend chaque règle
//! testable de façon déterministe.

use std::collections::VecDeque;

use serde::Serialize;

use crate::alert::AlertPacket;

/// Âge maximal d'une alerte acceptée (anti-rejeu), comme en v2.5.
pub const MAX_ALERT_AGE_SECONDS: i64 = 120;

/// Nombre maximal d'alertes conservées pour l'affichage.
pub const INBOX_MAX_ITEMS: usize = 50;

/// Taille du cache de déduplication (identifiants déjà vus).
pub const DEDUP_CACHE_SIZE: usize = 200;

/// Alerte prête à afficher sur le portail.
///
/// Les noms de champs sont ceux du `lora_inbox.json` de la v2.5 :
/// l'`index.html` legacy les lit tels quels.
#[derive(Debug, Clone, Serialize)]
pub struct InboxAlert {
    /// Identifiant de déduplication ([`AlertPacket::unique_id`]).
    pub id: String,
    /// Nœud source.
    pub node_id: String,
    /// Type d'alerte (nom de fil).
    #[serde(rename = "type")]
    pub alert_type: &'static str,
    /// Libellé humain du type.
    pub type_label: &'static str,
    /// Message libre.
    pub message: String,
    /// Horodatage Unix d'émission.
    pub timestamp: i64,
    /// Heure d'émission formatée `HH:MM UTC` (compat portail v2.5).
    pub datetime: String,
    /// Nombre de rebonds mesh.
    pub hop: u8,
    /// Vrai si la signature Ed25519 a été vérifiée.
    pub verified: bool,
}

/// Verdict d'admission d'un paquet dans la boîte de réception.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// Paquet admis et ajouté.
    Accepted,
    /// Déjà reçu (même source + même horodatage).
    Duplicate,
    /// Trop ancien — rejeté (protection anti-rejeu).
    TooOld,
}

/// Boîte de réception en mémoire (les alertes ne survivent pas à un
/// redémarrage, conformément à la politique « zéro donnée persistante »).
#[derive(Debug, Default)]
pub struct AlertInbox {
    alerts: Vec<InboxAlert>,
    seen: VecDeque<String>,
}

impl AlertInbox {
    /// Crée une boîte vide.
    pub fn new() -> Self {
        Self {
            alerts: Vec::new(),
            seen: VecDeque::with_capacity(DEDUP_CACHE_SIZE),
        }
    }

    /// Applique les règles d'admission puis ajoute l'alerte si elle passe.
    ///
    /// `now` est l'horodatage Unix courant ; `verified` indique si la
    /// signature du paquet a été vérifiée par l'appelant.
    pub fn admit(&mut self, packet: &AlertPacket, verified: bool, now: i64) -> Admission {
        if packet.age_seconds(now) > MAX_ALERT_AGE_SECONDS {
            return Admission::TooOld;
        }

        let uid = packet.unique_id();
        if self.seen.contains(&uid) {
            return Admission::Duplicate;
        }
        if self.seen.len() == DEDUP_CACHE_SIZE {
            self.seen.pop_front();
        }
        self.seen.push_back(uid.clone());

        self.alerts.insert(
            0,
            InboxAlert {
                id: uid,
                node_id: packet.node_id.clone(),
                alert_type: packet.alert_type.wire_name(),
                type_label: packet.alert_type.label(),
                message: packet.message.clone(),
                timestamp: packet.timestamp,
                datetime: format_hh_mm_utc(packet.timestamp),
                hop: packet.hop,
                verified,
            },
        );
        self.alerts.truncate(INBOX_MAX_ITEMS);
        Admission::Accepted
    }

    /// Alertes affichables, la plus récente en premier.
    pub fn alerts(&self) -> &[InboxAlert] {
        &self.alerts
    }
}

/// Formate un horodatage Unix en `HH:MM UTC` (compat portail v2.5).
fn format_hh_mm_utc(timestamp: i64) -> String {
    let seconds_of_day = timestamp.rem_euclid(86_400);
    format!(
        "{:02}:{:02} UTC",
        seconds_of_day / 3_600,
        (seconds_of_day % 3_600) / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alert::AlertType;

    fn packet(node: &str, ts: i64) -> AlertPacket {
        AlertPacket::new(node, AlertType::Incendie, "feu", ts)
    }

    #[test]
    fn accepts_then_deduplicates() {
        let mut inbox = AlertInbox::new();
        let p = packet("nid", 1_000);
        assert_eq!(inbox.admit(&p, true, 1_010), Admission::Accepted);
        assert_eq!(inbox.admit(&p, true, 1_020), Admission::Duplicate);
        assert_eq!(inbox.alerts().len(), 1);
    }

    #[test]
    fn rejects_too_old() {
        let mut inbox = AlertInbox::new();
        let p = packet("nid", 1_000);
        assert_eq!(
            inbox.admit(&p, true, 1_000 + MAX_ALERT_AGE_SECONDS + 1),
            Admission::TooOld
        );
        assert!(inbox.alerts().is_empty());
    }

    #[test]
    fn newest_first_and_bounded() {
        let mut inbox = AlertInbox::new();
        for i in 0..(INBOX_MAX_ITEMS as i64 + 10) {
            inbox.admit(&packet(&format!("n{i}"), i), true, i);
        }
        assert_eq!(inbox.alerts().len(), INBOX_MAX_ITEMS);
        let newest = inbox.alerts().first().map(|a| a.timestamp);
        assert_eq!(newest, Some(INBOX_MAX_ITEMS as i64 + 9));
    }

    #[test]
    fn dedup_cache_is_bounded() {
        let mut inbox = AlertInbox::new();
        for i in 0..(DEDUP_CACHE_SIZE as i64 + 50) {
            inbox.admit(&packet("same-node", i), true, i);
        }
        assert_eq!(inbox.seen.len(), DEDUP_CACHE_SIZE);
    }

    #[test]
    fn formats_datetime_like_v2_5() {
        // 1748123456 → 21:50 UTC (vérifié avec datetime.fromtimestamp(tz=utc))
        assert_eq!(format_hh_mm_utc(1_748_123_456), "21:50 UTC");
        assert_eq!(format_hh_mm_utc(0), "00:00 UTC");
    }
}
