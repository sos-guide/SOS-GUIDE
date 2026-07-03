//! Signature/vérification **détachées** Ed25519 sur des octets arbitraires.
//!
//! Sert au manifeste de version ([`sos_core::VersionManifest`]) : une **clé de
//! publication** (distincte de l'identité du nœud) signe la charge canonique du
//! manifeste ; chaque nœud vérifie avec la clé **publique** de publication, de
//! confiance, avant d'appliquer une mise à jour. Clés au format PEM (PKCS#8
//! privée, SPKI publique), signature en base64 — cohérent avec le reste de
//! `sos-security`.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use ed25519_dalek::pkcs8::{DecodePrivateKey, DecodePublicKey};
use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};

use crate::keyring::{KeyError, VerifyError};

/// Signe `message` avec la clé privée PEM (PKCS#8) ; renvoie la signature base64.
pub fn sign_detached(private_key_pem: &str, message: &[u8]) -> Result<String, KeyError> {
    let signing = SigningKey::from_pkcs8_pem(private_key_pem)
        .map_err(|e| KeyError::InvalidPem(e.to_string()))?;
    Ok(BASE64.encode(signing.sign(message).to_bytes()))
}

/// Vérifie une signature détachée base64 sur `message` avec la clé publique PEM
/// (SPKI). `Ok(())` si la signature est valide et correspond à la clé.
pub fn verify_detached(
    public_key_pem: &str,
    message: &[u8],
    signature_b64: &str,
) -> Result<(), VerifyError> {
    let verifying =
        VerifyingKey::from_public_key_pem(public_key_pem).map_err(|_| VerifyError::BadEncoding)?;
    let sig_bytes: [u8; 64] = BASE64
        .decode(signature_b64)
        .map_err(|_| VerifyError::BadEncoding)?
        .try_into()
        .map_err(|_| VerifyError::BadEncoding)?;
    verifying
        .verify(message, &Signature::from_bytes(&sig_bytes))
        .map_err(|_| VerifyError::InvalidSignature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KeyRing;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn sign_then_verify_roundtrip() -> TestResult {
        // Une « clé de publication » : on réutilise KeyRing pour générer une paire.
        let release = KeyRing::generate("release");
        let priv_pem = release.private_key_pem()?;
        let pub_pem = release.public_key_pem()?;

        let msg = b"0.1.0|deadbeef|2026-06-22";
        let sig = sign_detached(&priv_pem, msg)?;
        assert!(verify_detached(&pub_pem, msg, &sig).is_ok());
        Ok(())
    }

    #[test]
    fn tampered_message_is_rejected() -> TestResult {
        let release = KeyRing::generate("release");
        let sig = sign_detached(&release.private_key_pem()?, b"message-original")?;
        assert_eq!(
            verify_detached(&release.public_key_pem()?, b"message-altere", &sig),
            Err(VerifyError::InvalidSignature)
        );
        Ok(())
    }

    #[test]
    fn wrong_key_is_rejected() -> TestResult {
        let signer = KeyRing::generate("release");
        let other = KeyRing::generate("imposteur");
        let sig = sign_detached(&signer.private_key_pem()?, b"m")?;
        assert_eq!(
            verify_detached(&other.public_key_pem()?, b"m", &sig),
            Err(VerifyError::InvalidSignature)
        );
        Ok(())
    }

    #[test]
    fn malformed_signature_is_rejected() -> TestResult {
        let release = KeyRing::generate("release");
        assert_eq!(
            verify_detached(&release.public_key_pem()?, b"m", "pas-du-base64-valide!"),
            Err(VerifyError::BadEncoding)
        );
        Ok(())
    }
}
