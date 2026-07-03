//! Manifeste de version signé — intégrité et authenticité des mises à jour.
//!
//! Une mise à jour binaire est décrite par un manifeste : version, empreinte
//! SHA-256 du binaire, date de build, et **signature Ed25519** de la charge
//! `version|sha256|builtAt`. Avant d'appliquer une MAJ, le nœud vérifie (1) que
//! l'empreinte du binaire reçu correspond au manifeste, (2) que la signature est
//! celle d'une **clé de publication de confiance** — empêchant l'installation
//! d'un binaire altéré ou non autorisé.
//!
//! Ce module est **pur** : il décrit le manifeste, calcule l'empreinte et fournit
//! la charge à signer ; la vérification cryptographique vit dans `sos-security`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Erreur d'analyse d'un manifeste de version.
#[derive(Debug, thiserror::Error)]
pub enum VersionError {
    /// JSON invalide ou champ obligatoire manquant.
    #[error("manifeste de version invalide : {0}")]
    Invalid(String),
}

/// Manifeste décrivant une version du binaire du nœud.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionManifest {
    /// Version sémantique du binaire (ex. `0.1.0`).
    pub version: String,
    /// Empreinte SHA-256 du binaire, en hexadécimal minuscule.
    pub sha256: String,
    /// Date de build (texte libre, ex. RFC 3339), couverte par la signature.
    #[serde(rename = "builtAt", default)]
    pub built_at: String,
    /// Signature Ed25519 (base64) de [`VersionManifest::canonical_payload`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// Empreinte SHA-256 d'un contenu, en hexadécimal minuscule (64 caractères).
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for byte in digest {
        // Deux hexdigits par octet, sans indexation.
        for nibble in [byte >> 4, byte & 0x0f] {
            hex.push(char::from_digit(u32::from(nibble), 16).unwrap_or('0'));
        }
    }
    hex
}

impl VersionManifest {
    /// Construit un manifeste **non signé** pour `binary` (empreinte calculée).
    #[must_use]
    pub fn for_binary(version: &str, built_at: &str, binary: &[u8]) -> Self {
        Self {
            version: version.to_owned(),
            sha256: sha256_hex(binary),
            built_at: built_at.to_owned(),
            signature: None,
        }
    }

    /// Charge signée/vérifiée : `version|sha256|builtAt` (la signature elle-même
    /// n'est pas couverte). Doit rester stable octet pour octet.
    #[must_use]
    pub fn canonical_payload(&self) -> Vec<u8> {
        format!("{}|{}|{}", self.version, self.sha256, self.built_at).into_bytes()
    }

    /// Vrai si l'empreinte du manifeste correspond au binaire fourni
    /// (comparaison insensible à la casse de l'hexadécimal).
    #[must_use]
    pub fn matches_binary(&self, binary: &[u8]) -> bool {
        sha256_hex(binary).eq_ignore_ascii_case(&self.sha256)
    }

    /// Sérialise le manifeste en JSON.
    pub fn to_json(&self) -> Result<String, VersionError> {
        serde_json::to_string(self).map_err(|e| VersionError::Invalid(e.to_string()))
    }

    /// Analyse un manifeste depuis son JSON.
    pub fn from_json(raw: &str) -> Result<Self, VersionError> {
        serde_json::from_str(raw).map_err(|e| VersionError::Invalid(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn sha256_matches_known_vector() {
        // SHA-256("abc") — vecteur de référence FIPS 180-4.
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn manifest_round_trip_and_hash_match() -> TestResult {
        let binary = b"binaire-de-test";
        let m = VersionManifest::for_binary("0.1.0", "2026-06-22T00:00:00Z", binary);
        assert!(m.matches_binary(binary));
        assert!(!m.matches_binary(b"autre"));
        let decoded = VersionManifest::from_json(&m.to_json()?)?;
        assert_eq!(decoded, m);
        Ok(())
    }

    #[test]
    fn canonical_payload_is_stable() {
        let m = VersionManifest {
            version: "0.1.0".to_owned(),
            sha256: "deadbeef".to_owned(),
            built_at: "2026-06-22".to_owned(),
            signature: Some("ignorée".to_owned()),
        };
        assert_eq!(m.canonical_payload(), b"0.1.0|deadbeef|2026-06-22");
    }

    #[test]
    fn missing_required_field_is_rejected() {
        assert!(VersionManifest::from_json(r#"{"version":"0.1.0"}"#).is_err());
    }
}
