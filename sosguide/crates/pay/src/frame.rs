//! Fragmentation d'une transaction en **trames LoRa** et réassemblage.
//!
//! Une transaction (jusqu'à [`crate::tx::MAX_TX_BYTES`]) dépasse la charge utile
//! d'une trame LoRa (~200 o). On la découpe donc en fragments numérotés, chacun
//! sérialisé en **JSON compact** (format de trame hérité de la v2.5), puis on les
//! réassemble à l'arrivée. Le codec est **pur et testé** ; l'émission réelle sur
//! le matériel LoRa relève de l'orchestrateur `live` (différé).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::tx::{hex_decode, hex_encode, PayTx};

/// Charge utile (octets bruts) par fragment, avant encodage hex + enveloppe JSON.
/// Choisie sous la limite pratique d'une trame LoRa/Meshtastic (~200 o utiles).
pub const FRAG_PAYLOAD: usize = 120;

/// Longueur de l'identifiant court de transaction porté par chaque fragment
/// (préfixe de l'empreinte locale — suffisant pour regrouper les fragments).
const SHORT_ID_LEN: usize = 16;

/// Erreur du codec de fragments.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum FrameError {
    /// Sérialisation JSON impossible.
    #[error("encodage de trame impossible")]
    Encode,
    /// Trame JSON illisible.
    #[error("trame illisible")]
    Decode,
    /// Charge hexadécimale d'un fragment invalide.
    #[error("charge de fragment invalide")]
    BadPayload,
    /// Fragment incohérent (total nul, index hors bornes, ou total divergent).
    #[error("fragment incohérent")]
    Inconsistent,
}

/// Un fragment de transaction prêt à voyager sur une trame LoRa.
///
/// Champs volontairement courts (JSON compact, LoRa avare en octets) : `i` = id
/// court, `n` = index, `t` = total, `d` = charge hex.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fragment {
    /// Identifiant court de la transaction (regroupe ses fragments).
    #[serde(rename = "i")]
    pub tx_id: String,
    /// Index du fragment (0-based).
    #[serde(rename = "n")]
    pub index: u16,
    /// Nombre total de fragments de la transaction.
    #[serde(rename = "t")]
    pub total: u16,
    /// Charge utile du fragment, en hexadécimal.
    #[serde(rename = "d")]
    pub data_hex: String,
}

/// Identifiant court dérivé de l'identifiant local complet.
#[must_use]
fn short_id(id: &str) -> String {
    id.chars().take(SHORT_ID_LEN).collect()
}

/// Découpe une transaction en fragments (jamais vide : une tx validée a ≥ 1 octet).
#[must_use]
pub fn fragment(tx: &PayTx) -> Vec<Fragment> {
    let id = short_id(tx.id());
    let chunks: Vec<&[u8]> = tx.raw().chunks(FRAG_PAYLOAD).collect();
    let total = chunks.len() as u16;
    chunks
        .iter()
        .enumerate()
        .map(|(index, chunk)| Fragment {
            tx_id: id.clone(),
            index: index as u16,
            total,
            data_hex: hex_encode(chunk),
        })
        .collect()
}

/// Sérialise un fragment en trame JSON compacte (à poser sur une trame LoRa).
pub fn encode_frame(frag: &Fragment) -> Result<String, FrameError> {
    serde_json::to_string(frag).map_err(|_| FrameError::Encode)
}

/// Désérialise une trame JSON reçue en fragment.
pub fn decode_frame(wire: &str) -> Result<Fragment, FrameError> {
    serde_json::from_str(wire).map_err(|_| FrameError::Decode)
}

/// Fragments en cours de réassemblage pour une transaction.
struct Partial {
    total: u16,
    parts: Vec<Option<Vec<u8>>>,
}

/// Réassemble les transactions à partir des fragments reçus (dans le désordre).
#[derive(Default)]
pub struct Reassembler {
    partial: HashMap<String, Partial>,
}

impl Reassembler {
    /// Nouveau réassembleur vide.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ingère un fragment. Renvoie `Some(octets)` dès que la transaction est
    /// **complète** (tous ses fragments reçus), sinon `None`.
    pub fn ingest(&mut self, frag: &Fragment) -> Result<Option<Vec<u8>>, FrameError> {
        if frag.total == 0 || frag.index >= frag.total {
            return Err(FrameError::Inconsistent);
        }
        let data = hex_decode(&frag.data_hex).map_err(|_| FrameError::BadPayload)?;

        let entry = self
            .partial
            .entry(frag.tx_id.clone())
            .or_insert_with(|| Partial {
                total: frag.total,
                parts: vec![None; frag.total as usize],
            });
        if entry.total != frag.total {
            return Err(FrameError::Inconsistent);
        }
        match entry.parts.get_mut(frag.index as usize) {
            Some(slot) => *slot = Some(data),
            None => return Err(FrameError::Inconsistent),
        }

        if entry.parts.iter().all(Option::is_some) {
            let mut raw = Vec::new();
            for chunk in entry.parts.iter().flatten() {
                raw.extend_from_slice(chunk);
            }
            self.partial.remove(&frag.tx_id);
            return Ok(Some(raw));
        }
        Ok(None)
    }

    /// Nombre de transactions partiellement reçues (en attente de fragments).
    #[must_use]
    pub fn pending(&self) -> usize {
        self.partial.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type R = Result<(), Box<dyn std::error::Error>>;

    fn sample_tx(bytes: usize) -> Result<PayTx, Box<dyn std::error::Error>> {
        let hex = "ab".repeat(bytes);
        Ok(PayTx::parse_hex(&hex)?)
    }

    #[test]
    fn fragments_cover_the_whole_tx() -> R {
        let tx = sample_tx(300)?; // 300 o → 3 fragments (120+120+60)
        let frags = fragment(&tx);
        assert_eq!(frags.len(), 3);
        assert!(frags.iter().all(|f| f.total == 3));
        let carried: usize = frags.iter().map(|f| f.data_hex.len() / 2).sum();
        assert_eq!(carried, 300);
        Ok(())
    }

    #[test]
    fn reassembles_out_of_order() -> R {
        let tx = sample_tx(250)?;
        let frags = fragment(&tx);
        let mut re = Reassembler::new();
        // Ingestion dans le désordre : 2, 0, 1.
        let mut done = None;
        for i in [2usize, 0, 1] {
            let frag = frags.get(i).ok_or("fragment manquant")?;
            if let Some(raw) = re.ingest(frag)? {
                done = Some(raw);
            }
        }
        assert_eq!(done.as_deref(), Some(tx.raw()));
        assert_eq!(re.pending(), 0);
        Ok(())
    }

    #[test]
    fn incomplete_yields_none() -> R {
        let tx = sample_tx(300)?;
        let frags = fragment(&tx);
        let mut re = Reassembler::new();
        // On n'ingère que 2 des 3 fragments.
        let f0 = frags.first().ok_or("f0")?;
        let f1 = frags.get(1).ok_or("f1")?;
        assert_eq!(re.ingest(f0)?, None);
        assert_eq!(re.ingest(f1)?, None);
        assert_eq!(re.pending(), 1);
        Ok(())
    }

    #[test]
    fn wire_round_trip_and_bad_frames() -> R {
        let tx = sample_tx(50)?;
        let frags = fragment(&tx);
        let frag = frags.first().ok_or("frag")?;
        let wire = encode_frame(frag)?;
        assert_eq!(decode_frame(&wire)?, *frag);
        assert_eq!(decode_frame("pas du json"), Err(FrameError::Decode));
        Ok(())
    }

    #[test]
    fn rejects_inconsistent_fragment() {
        let mut re = Reassembler::new();
        let bad = Fragment {
            tx_id: "x".to_owned(),
            index: 0,
            total: 0,
            data_hex: "ab".to_owned(),
        };
        assert_eq!(re.ingest(&bad), Err(FrameError::Inconsistent));
    }
}
