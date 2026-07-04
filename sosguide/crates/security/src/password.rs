//! Mot de passe administrateur : sel aléatoire + renforcement **type PBKDF2**
//! (SHA-256, mot de passe ré-injecté à chaque itération), comparaison à temps
//! constant.
//!
//! L'administration est locale et hors-Internet (un seul admin sur l'AP du nœud),
//! mais le mot de passe ne doit jamais être stocké en clair ni comparé de façon
//! naïve. Le renforcement par itérations relève le coût d'une attaque par force
//! brute hors-ligne sur la base Redb.
//!
//! *Argon2 (mémoire-dur ; `argon2` RustCrypto = pur Rust, sans dépendance C) serait
//! plus résistant au GPU/ASIC. Il n'est volontairement pas employé ici car
//! [`verify_password`] sert aussi le chemin **chaud** `/api/ping` (clés de groupe
//! testées une par une) : on garde un coût **CPU pur**, sans amplification mémoire
//! exploitable en déni de service. Un chemin Argon2 dédié au seul mot de passe
//! admin reste envisageable.*

use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};

/// Itérations pour le **mot de passe administrateur** : élevé (secret de haute
/// valeur, vérifié rarement, au login).
const ITERATIONS: u32 = 100_000;
/// Itérations pour les **clés de groupe de ping** : bien plus faibles. Ces clés
/// sont de **faible sensibilité** (émettre un ping anonyme dans un groupe) et sont
/// vérifiées sur le **chemin chaud** `/api/ping` (une par groupe) : un coût élevé
/// y créerait une amplification CPU exploitable en déni de service. Défense
/// proportionnée à la valeur du secret + tenable sur le chemin chaud.
const GROUP_ITERATIONS: u32 = 1_000;
/// Longueur du sel aléatoire, en octets.
const SALT_LEN: usize = 16;

/// Empreinte d'un mot de passe : sel et empreinte renforcée, en hexadécimal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordHash {
    /// Sel aléatoire (hex).
    pub salt_hex: String,
    /// Empreinte renforcée (hex, 32 octets).
    pub hash_hex: String,
}

/// Calcule l'empreinte du **mot de passe administrateur** (renforcement fort).
#[must_use]
pub fn hash_password(password: &str) -> PasswordHash {
    hash_with(password, ITERATIONS)
}

/// Vérifie le **mot de passe administrateur** (temps constant ; entrée malformée → `false`).
#[must_use]
pub fn verify_password(password: &str, salt_hex: &str, hash_hex: &str) -> bool {
    verify_with(password, salt_hex, hash_hex, ITERATIONS)
}

/// Calcule l'empreinte d'une **clé de groupe de ping** (renforcement léger).
#[must_use]
pub fn hash_group_key(key: &str) -> PasswordHash {
    hash_with(key, GROUP_ITERATIONS)
}

/// Vérifie une **clé de groupe de ping** (temps constant, coût faible pour `/api/ping`).
#[must_use]
pub fn verify_group_key(key: &str, salt_hex: &str, hash_hex: &str) -> bool {
    verify_with(key, salt_hex, hash_hex, GROUP_ITERATIONS)
}

/// Hachage salé avec un nombre d'itérations paramétré.
fn hash_with(secret: &str, iterations: u32) -> PasswordHash {
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let digest = stretch(secret.as_bytes(), &salt, iterations);
    PasswordHash {
        salt_hex: to_hex(&salt),
        hash_hex: to_hex(&digest),
    }
}

/// Vérification à temps constant avec itérations paramétrées ; entrée malformée → `false`.
fn verify_with(secret: &str, salt_hex: &str, hash_hex: &str, iterations: u32) -> bool {
    let Some(salt) = from_hex(salt_hex) else {
        return false;
    };
    let Some(expected) = from_hex(hash_hex) else {
        return false;
    };
    let digest = stretch(secret.as_bytes(), &salt, iterations);
    constant_time_eq(&digest, &expected)
}

/// Renforcement **type PBKDF2** : à chaque itération, on hache
/// `SHA-256(digest précédent || salt || password)`. Le mot de passe **et** le sel
/// restent injectés du début à la fin (corrige la construction naïve où le mot de
/// passe n'entrait qu'une fois, puis où l'on ne re-hachait que le digest — la
/// force brute pouvait alors travailler sur une simple chaîne de hachage).
fn stretch(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut digest: [u8; 32] = {
        let mut hasher = Sha256::new();
        hasher.update(salt);
        hasher.update(password);
        hasher.finalize().into()
    };
    for _ in 1..iterations {
        let mut hasher = Sha256::new();
        hasher.update(digest);
        hasher.update(salt);
        hasher.update(password);
        digest = hasher.finalize().into();
    }
    digest
}

/// Comparaison à temps constant (indépendante de la position du 1er octet
/// divergent) pour ne pas fuiter d'information par timing.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        out.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    out
}

fn from_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    let bytes = hex.as_bytes();
    let mut out = Vec::with_capacity(hex.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let hi = (*pair.first()? as char).to_digit(16)?;
        let lo = (*pair.get(1)? as char).to_digit(16)?;
        out.push(u8::try_from((hi << 4) | lo).ok()?);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_key_roundtrip_and_wrong_key_rejected() {
        // Clé de groupe : hachage léger dédié, même contrat (round-trip + rejet).
        let h = hash_group_key("clé-de-groupe");
        assert!(verify_group_key("clé-de-groupe", &h.salt_hex, &h.hash_hex));
        assert!(!verify_group_key("mauvaise", &h.salt_hex, &h.hash_hex));
        // Un hachage admin (itérations différentes) ne valide PAS comme clé de groupe.
        let admin = hash_password("clé-de-groupe");
        assert!(!verify_group_key(
            "clé-de-groupe",
            &admin.salt_hex,
            &admin.hash_hex
        ));
    }

    #[test]
    fn hash_then_verify_roundtrip() {
        let h = hash_password("correct horse battery staple");
        assert!(verify_password(
            "correct horse battery staple",
            &h.salt_hex,
            &h.hash_hex
        ));
        assert!(!verify_password("mauvais", &h.salt_hex, &h.hash_hex));
    }

    #[test]
    fn distinct_salts_give_distinct_hashes() {
        let a = hash_password("même mot de passe");
        let b = hash_password("même mot de passe");
        assert_ne!(a.salt_hex, b.salt_hex);
        assert_ne!(a.hash_hex, b.hash_hex);
    }

    #[test]
    fn malformed_hex_is_rejected() {
        assert!(!verify_password("x", "zz", "00"));
        assert!(!verify_password("x", "00", "abc")); // longueur impaire
    }

    #[test]
    fn hex_roundtrip() {
        let bytes = [0u8, 15, 16, 255, 128];
        assert_eq!(from_hex(&to_hex(&bytes)), Some(bytes.to_vec()));
    }
}
