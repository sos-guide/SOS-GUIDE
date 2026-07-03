//! Identité Ed25519 du nœud et registre des nœuds de confiance.
//!
//! Interopérable avec la v2.5 : clés au format PEM (PKCS#8 pour la privée,
//! SPKI pour les publiques) et signatures base64 dans les trames — un nœud
//! Rust vérifie les alertes d'un nœud Python legacy et réciproquement.

use std::collections::HashMap;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use ed25519_dalek::pkcs8::spki::der::pem::LineEnding;
use ed25519_dalek::pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey};
use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};
use serde::Deserialize;
use sos_core::AlertPacket;

/// Erreurs de gestion de clés et de signatures.
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    /// Clé PEM illisible ou d'un mauvais format.
    #[error("clé PEM invalide : {0}")]
    InvalidPem(String),
    /// Le fichier `trusted_nodes.json` est illisible ou malformé.
    #[error("trusted_nodes.json invalide : {0}")]
    InvalidTrustedNodes(String),
}

/// Raisons de rejet d'une signature de paquet.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum VerifyError {
    /// Le paquet ne porte aucune signature.
    #[error("signature absente")]
    MissingSignature,
    /// Le nœud source n'est pas dans le registre de confiance.
    #[error("nœud « {0} » inconnu du registre de confiance")]
    UnknownNode(String),
    /// La signature n'est pas un base64 de 64 octets.
    #[error("encodage de signature invalide")]
    BadEncoding,
    /// La signature ne correspond pas à la charge signée.
    #[error("signature Ed25519 invalide")]
    InvalidSignature,
}

/// Format du fichier `trusted_nodes.json` hérité de la v2.5 :
/// `{"nodes": {"<node_id>": {"public_key": "-----BEGIN PUBLIC KEY-----…"}}}`.
#[derive(Deserialize)]
struct TrustedNodesFile {
    #[serde(default)]
    nodes: HashMap<String, TrustedNodeEntry>,
}

#[derive(Deserialize)]
struct TrustedNodeEntry {
    #[serde(default)]
    public_key: String,
}

/// Trousseau du nœud : sa clé de signature et les clés publiques
/// des nœuds dont il accepte les alertes.
pub struct KeyRing {
    node_id: String,
    signing: SigningKey,
    trusted: HashMap<String, VerifyingKey>,
}

impl KeyRing {
    /// Génère une identité neuve (clé Ed25519 aléatoire).
    ///
    /// Le nœud fait automatiquement confiance à sa propre clé, comme la
    /// v2.5 (« seule la signature de ce nœud sera acceptée » par défaut).
    pub fn generate(node_id: &str) -> Self {
        let signing = SigningKey::generate(&mut rand_core::OsRng);
        Self::from_signing_key(node_id, signing)
    }

    /// Charge l'identité depuis une clé privée PEM PKCS#8 de la v2.5
    /// (`/etc/sos-guide/node_private_key.pem`).
    pub fn from_private_key_pem(node_id: &str, pem: &str) -> Result<Self, KeyError> {
        let signing =
            SigningKey::from_pkcs8_pem(pem).map_err(|e| KeyError::InvalidPem(e.to_string()))?;
        Ok(Self::from_signing_key(node_id, signing))
    }

    fn from_signing_key(node_id: &str, signing: SigningKey) -> Self {
        let mut trusted = HashMap::new();
        trusted.insert(node_id.to_owned(), signing.verifying_key());
        Self {
            node_id: node_id.to_owned(),
            signing,
            trusted,
        }
    }

    /// Identifiant de ce nœud.
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    /// Clé privée au format PEM PKCS#8 (à persister avec des droits 0400).
    pub fn private_key_pem(&self) -> Result<String, KeyError> {
        self.signing
            .to_pkcs8_pem(LineEnding::LF)
            .map(|pem| pem.to_string())
            .map_err(|e| KeyError::InvalidPem(e.to_string()))
    }

    /// Clé publique au format PEM SPKI (partageable, pour `trusted_nodes.json`).
    pub fn public_key_pem(&self) -> Result<String, KeyError> {
        self.signing
            .verifying_key()
            .to_public_key_pem(LineEnding::LF)
            .map_err(|e| KeyError::InvalidPem(e.to_string()))
    }

    /// Charge le registre de confiance depuis le contenu de
    /// `trusted_nodes.json` (rechargeable à chaud). Les entrées invalides
    /// sont ignorées ; retourne le nombre de nœuds chargés.
    pub fn load_trusted_nodes(&mut self, json: &str) -> Result<usize, KeyError> {
        let file: TrustedNodesFile =
            serde_json::from_str(json).map_err(|e| KeyError::InvalidTrustedNodes(e.to_string()))?;

        let mut loaded = 0;
        for (nid, entry) in file.nodes {
            if entry.public_key.is_empty() {
                continue;
            }
            if let Ok(key) = VerifyingKey::from_public_key_pem(&entry.public_key) {
                self.trusted.insert(nid, key);
                loaded += 1;
            }
        }
        Ok(loaded)
    }

    /// Réinitialise le registre de confiance à **la seule clé du nœud lui-même**.
    /// Utilisé avant un rechargement complet (« remplacer le registre ») pour que
    /// les entrées supprimées côté admin ne subsistent pas en mémoire.
    pub fn clear_trusted_nodes(&mut self) {
        self.trusted.clear();
        self.trusted
            .insert(self.node_id.clone(), self.signing.verifying_key());
    }

    /// Identifiants des nœuds de confiance (y compris ce nœud), triés. Pour
    /// l'affichage administrateur du registre.
    pub fn trusted_node_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.trusted.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// Signe la charge `v|nid|typ|ts|msg` du paquet et retourne la
    /// signature en base64, prête à être placée dans la trame.
    pub fn sign(&self, packet: &AlertPacket) -> String {
        let signature = self.signing.sign(&packet.payload_to_sign());
        BASE64.encode(signature.to_bytes())
    }

    /// Vérifie la signature d'un paquet reçu contre le registre de confiance.
    pub fn verify(&self, packet: &AlertPacket) -> Result<(), VerifyError> {
        let sig_b64 = packet
            .signature
            .as_deref()
            .ok_or(VerifyError::MissingSignature)?;

        let public_key = self
            .trusted
            .get(&packet.node_id)
            .ok_or_else(|| VerifyError::UnknownNode(packet.node_id.clone()))?;

        let sig_bytes: [u8; 64] = BASE64
            .decode(sig_b64)
            .map_err(|_| VerifyError::BadEncoding)?
            .try_into()
            .map_err(|_| VerifyError::BadEncoding)?;

        public_key
            .verify(
                &packet.payload_to_sign(),
                &Signature::from_bytes(&sig_bytes),
            )
            .map_err(|_| VerifyError::InvalidSignature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sos_core::AlertType;

    fn signed_packet(ring: &KeyRing) -> AlertPacket {
        let mut packet = AlertPacket::new(ring.node_id(), AlertType::Crue, "montée des eaux", 42);
        packet.signature = Some(ring.sign(&packet));
        packet
    }

    #[test]
    fn sign_then_verify() {
        let ring = KeyRing::generate("nid-test");
        let packet = signed_packet(&ring);
        assert_eq!(ring.verify(&packet), Ok(()));
    }

    #[test]
    fn tampered_message_is_rejected() {
        let ring = KeyRing::generate("nid-test");
        let mut packet = signed_packet(&ring);
        packet.message.push('!');
        assert_eq!(ring.verify(&packet), Err(VerifyError::InvalidSignature));
    }

    #[test]
    fn hop_increment_keeps_signature_valid() {
        // hop n'est pas signé : un rebond mesh ne doit pas invalider l'alerte.
        let ring = KeyRing::generate("nid-test");
        let mut packet = signed_packet(&ring);
        packet.hop = 5;
        assert_eq!(ring.verify(&packet), Ok(()));
    }

    #[test]
    fn unknown_node_and_missing_signature_are_rejected() {
        let ring = KeyRing::generate("nid-test");
        let other = KeyRing::generate("autre-nid");
        let foreign = signed_packet(&other);
        assert_eq!(
            ring.verify(&foreign),
            Err(VerifyError::UnknownNode("autre-nid".to_owned()))
        );

        let mut unsigned = signed_packet(&ring);
        unsigned.signature = None;
        assert_eq!(ring.verify(&unsigned), Err(VerifyError::MissingSignature));
    }

    #[test]
    fn pem_round_trip_and_trusted_nodes_loading() -> Result<(), KeyError> {
        let ring = KeyRing::generate("nid-a");
        let reloaded = KeyRing::from_private_key_pem("nid-a", &ring.private_key_pem()?)?;
        let packet = signed_packet(&ring);
        assert_eq!(reloaded.verify(&packet), Ok(()));

        // Un troisième nœud apprend la clé publique de nid-a via le registre.
        let mut third = KeyRing::generate("nid-c");
        assert_eq!(
            third.verify(&packet),
            Err(VerifyError::UnknownNode("nid-a".to_owned()))
        );
        let registry = format!(
            r#"{{"nodes":{{"nid-a":{{"public_key":{}}},"vide":{{"public_key":""}}}}}}"#,
            serde_json::to_string(&ring.public_key_pem()?)
                .map_err(|e| KeyError::InvalidTrustedNodes(e.to_string()))?
        );
        assert_eq!(third.load_trusted_nodes(&registry)?, 1);
        assert_eq!(third.verify(&packet), Ok(()));
        Ok(())
    }

    #[test]
    fn clear_keeps_self_and_lists_ids() -> Result<(), KeyError> {
        let ring_a = KeyRing::generate("nid-a");
        let mut node = KeyRing::generate("nid-self");
        let registry = format!(
            r#"{{"nodes":{{"nid-a":{{"public_key":{}}}}}}}"#,
            serde_json::to_string(&ring_a.public_key_pem()?)
                .map_err(|e| KeyError::InvalidTrustedNodes(e.to_string()))?
        );
        node.load_trusted_nodes(&registry)?;
        assert_eq!(node.trusted_node_ids(), vec!["nid-a", "nid-self"]);

        // Après réinitialisation, seul le nœud lui-même reste de confiance.
        node.clear_trusted_nodes();
        assert_eq!(node.trusted_node_ids(), vec!["nid-self"]);
        let packet = signed_packet(&ring_a);
        assert_eq!(
            node.verify(&packet),
            Err(VerifyError::UnknownNode("nid-a".to_owned()))
        );
        Ok(())
    }
}
