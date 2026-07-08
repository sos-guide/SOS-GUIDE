//! Transaction Bitcoin **signée et opaque** transportée par la borne.
//!
//! La borne ne signe rien et **ne détient aucune clé ni fonds** : un client signe
//! sa transaction sur son propre portefeuille, puis la remet à la borne sous forme
//! d'octets bruts (hex). La borne se contente de la mettre en file, de la fragmenter
//! pour le mesh LoRa et de la faire diffuser par un nœud-sortie connecté. La
//! validité *consensus* Bitcoin n'est **jamais** vérifiée ici (c'est le rôle du
//! réseau) : on ne contrôle que le format et la taille, pour protéger le canal LoRa.

use sos_core::sha256_hex;

/// Taille maximale d'une transaction acceptée (octets bruts). Une transaction
/// Bitcoin signée usuelle fait ~200–400 o ; on plafonne bas car, au-delà, le relais
/// mesh coûterait trop d'airtime LoRa (canal réservé **d'abord aux alertes**).
pub const MAX_TX_BYTES: usize = 2_000;

/// Statut d'une transaction dans la file de relais.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxStatus {
    /// Reçue et validée, en attente de relais.
    Queued,
    /// Émise sur le mesh LoRa (vers un nœud-sortie).
    Relayed,
    /// Diffusée sur le réseau Bitcoin par un nœud-sortie connecté.
    Broadcast,
}

/// Erreur de validation d'une transaction entrante.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TxError {
    /// Chaîne hexadécimale mal formée (longueur impaire ou caractère non hex).
    #[error("hexadécimal invalide")]
    BadHex,
    /// Transaction vide.
    #[error("transaction vide")]
    Empty,
    /// Transaction trop volumineuse pour un relais LoRa raisonnable.
    #[error("transaction trop volumineuse : {0} o (max {MAX_TX_BYTES} o)")]
    TooLarge(usize),
}

/// Valeur d'un chiffre hexadécimal, ou `None` si le caractère n'en est pas un.
fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Caractère hexadécimal minuscule pour les 4 bits de poids faible de `n`.
fn hex_char(n: u8) -> char {
    match n & 0x0f {
        d @ 0..=9 => char::from(b'0' + d),
        d => char::from(b'a' + d - 10),
    }
}

/// Décode une chaîne hexadécimale en octets (pur, sans dépendance ni indexation).
pub(crate) fn hex_decode(s: &str) -> Result<Vec<u8>, TxError> {
    let bytes = s.as_bytes();
    if bytes.len() % 2 != 0 {
        return Err(TxError::BadHex);
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let hi = pair
            .first()
            .copied()
            .and_then(nibble)
            .ok_or(TxError::BadHex)?;
        let lo = pair
            .get(1)
            .copied()
            .and_then(nibble)
            .ok_or(TxError::BadHex)?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

/// Encode des octets en chaîne hexadécimale minuscule (pur).
#[must_use]
pub fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(hex_char(b >> 4));
        s.push(hex_char(b & 0x0f));
    }
    s
}

/// Transaction signée en file : octets bruts + identifiant local + statut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayTx {
    id: String,
    raw: Vec<u8>,
    status: TxStatus,
}

impl PayTx {
    /// Valide et construit une transaction depuis sa représentation hexadécimale.
    /// **Ne vérifie pas la validité consensus Bitcoin** : seulement le format et
    /// la taille.
    pub fn parse_hex(hex: &str) -> Result<Self, TxError> {
        Self::from_raw(hex_decode(hex.trim())?)
    }

    /// Valide et construit une transaction depuis ses octets bruts (ex. tx
    /// réassemblée depuis des fragments reçus du maillage). Contrôle taille/vacuité.
    pub fn from_raw(raw: Vec<u8>) -> Result<Self, TxError> {
        if raw.is_empty() {
            return Err(TxError::Empty);
        }
        if raw.len() > MAX_TX_BYTES {
            return Err(TxError::TooLarge(raw.len()));
        }
        // Identifiant **local** de déduplication : empreinte du contenu brut. Ce
        // n'est PAS le txid réseau (qui exige de parser la structure segwit) —
        // juste une clé stable pour la file et l'anti-doublon.
        let id = sha256_hex(&raw);
        Ok(Self {
            id,
            raw,
            status: TxStatus::Queued,
        })
    }

    /// Identifiant local (empreinte du contenu, ≠ txid réseau).
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Octets bruts de la transaction signée.
    #[must_use]
    pub fn raw(&self) -> &[u8] {
        &self.raw
    }

    /// Représentation hexadécimale (pour la diffusion via l'API publique).
    #[must_use]
    pub fn hex(&self) -> String {
        hex_encode(&self.raw)
    }

    /// Taille en octets.
    #[must_use]
    pub fn len(&self) -> usize {
        self.raw.len()
    }

    /// `true` si la transaction est vide (jamais le cas après [`Self::parse_hex`]).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Statut courant dans la file.
    #[must_use]
    pub fn status(&self) -> TxStatus {
        self.status
    }

    /// Met à jour le statut (usage interne à la file).
    pub(crate) fn set_status(&mut self, status: TxStatus) {
        self.status = status;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type R = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn hex_round_trip() -> R {
        let bytes = [0x00u8, 0x01, 0xab, 0xff, 0x10];
        let s = hex_encode(&bytes);
        assert_eq!(s, "0001abff10");
        assert_eq!(hex_decode(&s)?, bytes);
        Ok(())
    }

    #[test]
    fn rejects_bad_hex_and_odd_length() {
        assert_eq!(PayTx::parse_hex("xyz"), Err(TxError::BadHex));
        assert_eq!(PayTx::parse_hex("abc"), Err(TxError::BadHex));
        assert_eq!(PayTx::parse_hex(""), Err(TxError::Empty));
    }

    #[test]
    fn rejects_oversized_tx() {
        // 2001 octets → 4002 caractères hex.
        let big = "ab".repeat(MAX_TX_BYTES + 1);
        assert_eq!(
            PayTx::parse_hex(&big),
            Err(TxError::TooLarge(MAX_TX_BYTES + 1))
        );
    }

    #[test]
    fn valid_tx_has_stable_id_and_hex() -> R {
        let tx = PayTx::parse_hex(" 0100000000AbCd ")?; // espaces + casse tolérés
        assert_eq!(tx.status(), TxStatus::Queued);
        assert_eq!(tx.hex(), "0100000000abcd");
        assert_eq!(tx.len(), 7);
        // Id déterministe : même contenu → même id.
        let tx2 = PayTx::parse_hex("0100000000abcd")?;
        assert_eq!(tx.id(), tx2.id());
        assert_eq!(tx.id().len(), 64); // sha256 hex
        Ok(())
    }
}
