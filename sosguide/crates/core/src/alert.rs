//! Paquet d'alerte circulant sur le réseau mesh LoRa.
//!
//! Port fidèle du protocole v1 de SOS-GUIDE v2.5 (`lora-service.py`) :
//! le format de trame JSON compact et la charge signée sont identiques
//! octet pour octet, afin qu'un nœud Rust et un nœud Python legacy
//! puissent coexister dans le même mesh.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Version du protocole de trame. Toute trame d'une autre version est rejetée.
pub const PROTOCOL_VERSION: u8 = 1;

/// Longueur maximale de l'identifiant de nœud (en caractères, comme en v2.5).
pub const MAX_NODE_ID_CHARS: usize = 48;

/// Longueur maximale du message libre (en caractères, comme en v2.5).
pub const MAX_MESSAGE_CHARS: usize = 80;

/// Types d'alerte du protocole v1.
///
/// Les noms de fil (`PPMS`, `FIN_ALERTE`, …) sont figés : ils font partie
/// de la trame signée et doivent rester identiques à la v2.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AlertType {
    /// Plan Particulier de Mise en Sécurité.
    Ppms,
    /// Attentat / menace armée.
    Attentat,
    /// Risque nucléaire, radiologique, biologique ou chimique.
    Nrbc,
    /// Incendie.
    Incendie,
    /// Inondation / crue.
    Crue,
    /// Séisme.
    Seisme,
    /// Évacuation immédiate.
    Evacuation,
    /// Fin d'alerte — retour à la normale.
    FinAlerte,
    /// Message d'urgence libre.
    Custom,
}

impl AlertType {
    /// Nom du type tel qu'il circule dans la trame (`"PPMS"`, `"FIN_ALERTE"`…).
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Ppms => "PPMS",
            Self::Attentat => "ATTENTAT",
            Self::Nrbc => "NRBC",
            Self::Incendie => "INCENDIE",
            Self::Crue => "CRUE",
            Self::Seisme => "SEISME",
            Self::Evacuation => "EVACUATION",
            Self::FinAlerte => "FIN_ALERTE",
            Self::Custom => "CUSTOM",
        }
    }

    /// Analyse un nom de fil. Retourne `None` pour un type inconnu.
    pub fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            "PPMS" => Some(Self::Ppms),
            "ATTENTAT" => Some(Self::Attentat),
            "NRBC" => Some(Self::Nrbc),
            "INCENDIE" => Some(Self::Incendie),
            "CRUE" => Some(Self::Crue),
            "SEISME" => Some(Self::Seisme),
            "EVACUATION" => Some(Self::Evacuation),
            "FIN_ALERTE" => Some(Self::FinAlerte),
            "CUSTOM" => Some(Self::Custom),
            _ => None,
        }
    }

    /// Libellé humain affiché sur le portail (identique à la v2.5).
    pub fn label(self) -> &'static str {
        match self {
            Self::Ppms => "⚠️ Plan Particulier de Mise en Sécurité",
            Self::Attentat => "🚨 Attentat / Menace armée",
            Self::Nrbc => "☢️ Risque Nucléaire / Radiologique / Biologique / Chimique",
            Self::Incendie => "🔥 Incendie",
            Self::Crue => "🌊 Inondation / Crue",
            Self::Seisme => "🌍 Séisme",
            Self::Evacuation => "🏃 Évacuation immédiate",
            Self::FinAlerte => "✅ Fin d'alerte — retour à la normale",
            Self::Custom => "📢 Message d'urgence",
        }
    }
}

/// Erreurs de décodage ou de validation d'une trame d'alerte.
#[derive(Debug, thiserror::Error)]
pub enum AlertError {
    /// La trame n'est pas un JSON valide ou un champ obligatoire manque.
    #[error("trame JSON invalide : {0}")]
    InvalidFrame(String),
    /// La version de protocole de la trame n'est pas supportée.
    #[error("version de protocole non supportée : {0}")]
    UnsupportedVersion(u8),
    /// Le type d'alerte n'est pas connu de ce nœud.
    #[error("type d'alerte inconnu : {0}")]
    UnknownType(String),
}

/// Représentation exacte de la trame JSON (ordre des champs figé,
/// identique à `AlertPacket.to_json()` de la v2.5).
#[derive(Serialize, Deserialize)]
struct WireFrame<'a> {
    v: u8,
    nid: Cow<'a, str>,
    typ: Cow<'a, str>,
    ts: i64,
    #[serde(default)]
    hop: u8,
    #[serde(default, skip_serializing_if = "str_is_empty")]
    msg: Cow<'a, str>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sig: Option<Cow<'a, str>>,
}

fn str_is_empty(s: &str) -> bool {
    s.is_empty()
}

/// Paquet d'alerte du mesh.
///
/// La signature Ed25519 (`sig`) couvre `v|nid|typ|ts|msg` — mais ni `hop`
/// (incrémenté à chaque rebond) ni `sig` elle-même. Taille de trame visée :
/// ≤ 250 octets (limite LoRa SF7 : 255 octets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertPacket {
    /// Identifiant du nœud source (tronqué à [`MAX_NODE_ID_CHARS`]).
    pub node_id: String,
    /// Type d'alerte.
    pub alert_type: AlertType,
    /// Horodatage Unix de l'émission (anti-rejeu).
    pub timestamp: i64,
    /// Message libre optionnel (tronqué à [`MAX_MESSAGE_CHARS`]).
    pub message: String,
    /// Nombre de rebonds mesh (0 = source originale). Non signé.
    pub hop: u8,
    /// Signature Ed25519 en base64 (~88 caractères), si le paquet est signé.
    pub signature: Option<String>,
}

/// Tronque `s` à `max` caractères Unicode (sémantique du `[:max]` Python).
pub(crate) fn truncate_chars(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((byte_idx, _)) => s.get(..byte_idx).unwrap_or(s),
        None => s,
    }
}

impl AlertPacket {
    /// Construit un paquet local (hop 0, non signé), en appliquant les
    /// limites de longueur du protocole.
    pub fn new(node_id: &str, alert_type: AlertType, message: &str, timestamp: i64) -> Self {
        Self {
            node_id: truncate_chars(node_id, MAX_NODE_ID_CHARS).to_owned(),
            alert_type,
            timestamp,
            message: truncate_chars(message, MAX_MESSAGE_CHARS).to_owned(),
            hop: 0,
            signature: None,
        }
    }

    /// Charge à signer/vérifier : `v|nid|typ|ts|msg`, encodée en UTF-8.
    ///
    /// Doit rester identique octet pour octet à la v2.5 — un seul espace
    /// de différence invalide toutes les signatures du mesh.
    pub fn payload_to_sign(&self) -> Vec<u8> {
        format!(
            "{}|{}|{}|{}|{}",
            PROTOCOL_VERSION,
            self.node_id,
            self.alert_type.wire_name(),
            self.timestamp,
            self.message
        )
        .into_bytes()
    }

    /// Identifiant de déduplication : 16 premiers hexdigits de
    /// `sha256("nid:ts")`. Même alerte relayée par deux voisins → même id.
    pub fn unique_id(&self) -> String {
        let digest = Sha256::digest(format!("{}:{}", self.node_id, self.timestamp).as_bytes());
        let mut id = String::with_capacity(16);
        for byte in digest.iter().take(8) {
            for nibble in [byte >> 4, byte & 0x0f] {
                id.push(char::from_digit(u32::from(nibble), 16).unwrap_or('0'));
            }
        }
        id
    }

    /// Âge du paquet en secondes par rapport à `now` (horodatage Unix).
    pub fn age_seconds(&self, now: i64) -> i64 {
        now.saturating_sub(self.timestamp)
    }

    /// Sérialise la trame JSON compacte pour transmission LoRa.
    pub fn to_frame(&self) -> Result<String, AlertError> {
        let frame = WireFrame {
            v: PROTOCOL_VERSION,
            nid: Cow::Borrowed(&self.node_id),
            typ: Cow::Borrowed(self.alert_type.wire_name()),
            ts: self.timestamp,
            hop: self.hop,
            msg: Cow::Borrowed(&self.message),
            sig: self.signature.as_deref().map(Cow::Borrowed),
        };
        serde_json::to_string(&frame).map_err(|e| AlertError::InvalidFrame(e.to_string()))
    }

    /// Décode et valide une trame JSON reçue du mesh.
    pub fn from_frame(raw: &str) -> Result<Self, AlertError> {
        let frame: WireFrame<'_> =
            serde_json::from_str(raw).map_err(|e| AlertError::InvalidFrame(e.to_string()))?;

        if frame.v != PROTOCOL_VERSION {
            return Err(AlertError::UnsupportedVersion(frame.v));
        }
        let alert_type = AlertType::from_wire_name(&frame.typ)
            .ok_or_else(|| AlertError::UnknownType(frame.typ.clone().into_owned()))?;

        Ok(Self {
            node_id: truncate_chars(&frame.nid, MAX_NODE_ID_CHARS).to_owned(),
            alert_type,
            timestamp: frame.ts,
            message: truncate_chars(&frame.msg, MAX_MESSAGE_CHARS).to_owned(),
            hop: frame.hop,
            signature: frame.sig.map(Cow::into_owned),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AlertPacket {
        AlertPacket {
            node_id: "ecole-a-paris-75001".to_owned(),
            alert_type: AlertType::Ppms,
            timestamp: 1_748_123_456,
            message: "Rester en classe".to_owned(),
            hop: 0,
            signature: None,
        }
    }

    #[test]
    fn frame_is_byte_identical_to_python_v2_5() -> Result<(), AlertError> {
        // Vecteur généré avec json.dumps(separators=(",",":")) sur la v2.5.
        let expected = r#"{"v":1,"nid":"ecole-a-paris-75001","typ":"PPMS","ts":1748123456,"hop":0,"msg":"Rester en classe"}"#;
        assert_eq!(sample().to_frame()?, expected);
        Ok(())
    }

    #[test]
    fn payload_matches_python_v2_5() {
        assert_eq!(
            sample().payload_to_sign(),
            b"1|ecole-a-paris-75001|PPMS|1748123456|Rester en classe"
        );
    }

    #[test]
    fn unique_id_matches_python_v2_5() {
        // hashlib.sha256(b"ecole-a-paris-75001:1748123456").hexdigest()[:16]
        assert_eq!(sample().unique_id(), "5bf00d8327b82a3a");
    }

    #[test]
    fn frame_round_trip() -> Result<(), AlertError> {
        let mut packet = sample();
        packet.signature = Some("c2lnbmF0dXJl".to_owned());
        packet.hop = 3;
        let decoded = AlertPacket::from_frame(&packet.to_frame()?)?;
        assert_eq!(decoded, packet);
        Ok(())
    }

    #[test]
    fn rejects_unknown_version_and_type() {
        let bad_version = r#"{"v":2,"nid":"n","typ":"PPMS","ts":1}"#;
        assert!(matches!(
            AlertPacket::from_frame(bad_version),
            Err(AlertError::UnsupportedVersion(2))
        ));

        let bad_type = r#"{"v":1,"nid":"n","typ":"OVNI","ts":1}"#;
        assert!(matches!(
            AlertPacket::from_frame(bad_type),
            Err(AlertError::UnknownType(t)) if t == "OVNI"
        ));
    }

    #[test]
    fn rejects_missing_required_field() {
        assert!(AlertPacket::from_frame(r#"{"v":1,"typ":"PPMS","ts":1}"#).is_err());
        assert!(AlertPacket::from_frame("pas du json").is_err());
    }

    #[test]
    fn truncation_counts_chars_like_python() {
        // 100 caractères multi-octets : la coupe doit compter les caractères,
        // pas les octets, et tomber sur une frontière UTF-8 valide.
        let long: String = "é".repeat(100);
        let packet = AlertPacket::new(&long, AlertType::Custom, &long, 0);
        assert_eq!(packet.node_id.chars().count(), MAX_NODE_ID_CHARS);
        assert_eq!(packet.message.chars().count(), MAX_MESSAGE_CHARS);
    }

    #[test]
    fn all_types_round_trip_wire_names() {
        for t in [
            AlertType::Ppms,
            AlertType::Attentat,
            AlertType::Nrbc,
            AlertType::Incendie,
            AlertType::Crue,
            AlertType::Seisme,
            AlertType::Evacuation,
            AlertType::FinAlerte,
            AlertType::Custom,
        ] {
            assert_eq!(AlertType::from_wire_name(t.wire_name()), Some(t));
        }
    }
}
