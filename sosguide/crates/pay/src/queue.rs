//! File d'attente des transactions à relayer (bornée, anti-doublon).
//!
//! En mémoire uniquement (aucun bail/donnée persistée côté transport). Les
//! transactions y transitent de [`TxStatus::Queued`] → [`TxStatus::Relayed`] →
//! [`TxStatus::Broadcast`].

use crate::tx::{PayTx, TxStatus};

/// Nombre maximal de transactions gardées en file (borne mémoire + anti-DoS).
pub const MAX_QUEUE: usize = 64;

/// Erreur de mise en file.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum QueueError {
    /// File pleine.
    #[error("file pleine (max {MAX_QUEUE})")]
    Full,
    /// Transaction déjà présente (même identifiant local).
    #[error("transaction déjà en file")]
    Duplicate,
}

/// File d'attente bornée des transactions signées.
#[derive(Debug, Default)]
pub struct TxQueue {
    txs: Vec<PayTx>,
}

impl TxQueue {
    /// Nouvelle file vide.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ajoute une transaction. Refuse les doublons (même id) et le dépassement
    /// de capacité.
    pub fn enqueue(&mut self, tx: PayTx) -> Result<(), QueueError> {
        if self.txs.iter().any(|t| t.id() == tx.id()) {
            return Err(QueueError::Duplicate);
        }
        if self.txs.len() >= MAX_QUEUE {
            return Err(QueueError::Full);
        }
        self.txs.push(tx);
        Ok(())
    }

    /// Met à jour le statut d'une transaction. Renvoie `false` si l'id est inconnu.
    pub fn mark(&mut self, id: &str, status: TxStatus) -> bool {
        for tx in &mut self.txs {
            if tx.id() == id {
                tx.set_status(status);
                return true;
            }
        }
        false
    }

    /// Transactions restant à diffuser (statut ≠ [`TxStatus::Broadcast`]).
    pub fn pending(&self) -> impl Iterator<Item = &PayTx> {
        self.txs.iter().filter(|t| t.status() != TxStatus::Broadcast)
    }

    /// Toutes les transactions (tous statuts confondus).
    #[must_use]
    pub fn all(&self) -> &[PayTx] {
        &self.txs
    }

    /// Nombre de transactions en file.
    #[must_use]
    pub fn len(&self) -> usize {
        self.txs.len()
    }

    /// `true` si la file est vide.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.txs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type R = Result<(), Box<dyn std::error::Error>>;

    fn tx(byte: &str) -> Result<PayTx, Box<dyn std::error::Error>> {
        Ok(PayTx::parse_hex(byte)?)
    }

    #[test]
    fn enqueue_dedup_and_status_flow() -> R {
        let mut q = TxQueue::new();
        q.enqueue(tx("aabb")?)?;
        assert_eq!(q.len(), 1);
        // Doublon (même contenu → même id) : refusé.
        assert_eq!(q.enqueue(tx("aabb")?), Err(QueueError::Duplicate));
        // Autre tx : acceptée.
        q.enqueue(tx("ccdd")?)?;
        assert_eq!(q.len(), 2);
        assert_eq!(q.pending().count(), 2);

        // Marque l'une diffusée → elle sort des « pending ».
        let id = tx("aabb")?.id().to_owned();
        assert!(q.mark(&id, TxStatus::Broadcast));
        assert_eq!(q.pending().count(), 1);
        assert!(!q.mark("id-inconnu", TxStatus::Relayed));
        Ok(())
    }

    #[test]
    fn enforces_capacity() -> R {
        let mut q = TxQueue::new();
        for i in 0..MAX_QUEUE {
            // Contenus distincts → ids distincts.
            let hex = format!("{i:04x}");
            q.enqueue(PayTx::parse_hex(&hex)?)?;
        }
        assert_eq!(q.len(), MAX_QUEUE);
        assert_eq!(q.enqueue(tx("ffff")?), Err(QueueError::Full));
        Ok(())
    }
}
