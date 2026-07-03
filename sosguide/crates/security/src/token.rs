//! Génération de jetons aléatoires **lisibles** (jetons de session, identifiants
//! courts).
//!
//! Alphabet sans glyphes ambigus (pas de `0/O`, `1/l/I`) pour une recopie
//! humaine fiable.

use rand_core::{OsRng, RngCore};

/// Alphabet lisible (56 caractères), sans `0 O o 1 l I`.
const READABLE: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789";

/// Génère un jeton aléatoire de `len` caractères pris dans l'alphabet lisible.
/// Échantillonnage par rejet pour éliminer le biais modulo.
#[must_use]
pub fn random_token(len: usize) -> String {
    let n = READABLE.len() as u32;
    // Plus grand multiple de `n` tenant dans un u32 : au-delà, on rejette.
    let limit = u32::MAX - (u32::MAX % n);
    let mut out = String::with_capacity(len);
    while out.len() < len {
        let r = OsRng.next_u32();
        if r >= limit {
            continue;
        }
        if let Some(&b) = READABLE.get((r % n) as usize) {
            out.push(char::from(b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_has_requested_length() {
        assert_eq!(random_token(0).len(), 0);
        assert_eq!(random_token(12).len(), 12);
    }

    #[test]
    fn token_uses_only_readable_charset() {
        let t = random_token(200);
        assert!(t.bytes().all(|b| READABLE.contains(&b)));
        // Aucun glyphe ambigu.
        assert!(!t.contains(['0', 'O', 'o', '1', 'l', 'I']));
    }

    #[test]
    fn tokens_differ() {
        // Collision sur 12 caractères d'un alphabet de 56 : improbable.
        assert_ne!(random_token(12), random_token(12));
    }
}
