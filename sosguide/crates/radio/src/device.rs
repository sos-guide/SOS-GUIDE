//! Pilote radio matériel réel — **différé**.
//!
//! Le maillage LoRa cible deux transports (cf. ROADMAP Phase 3) :
//! - **SX1276** sur SPI (LoRa brut) ;
//! - **Meshtastic T-Beam** sur USB série.
//!
//! Aucun de ces périphériques n'est présent sur le Pi de développement, et le
//! choix du pilote (crate SPI/série, protocole) n'est pas tranché. On expose donc
//! un point d'entrée stable mais **inopérant** : tenter de l'ouvrir échoue
//! proprement (jamais de panique), et le mode `live` retombe sur un no-op
//! journalisé. Le vrai pilote viendra quand le matériel sera choisi et branché.

use crate::RadioError;

/// Tente d'ouvrir le périphérique radio série/SPI désigné par `path`.
///
/// **Non implémenté** tant que le matériel LoRa n'est pas choisi/branché :
/// retourne toujours [`RadioError::DeviceUnavailable`].
pub async fn open(path: &str) -> Result<(), RadioError> {
    Err(RadioError::DeviceUnavailable(path.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_is_deferred_and_fails_cleanly() {
        assert!(matches!(
            open("/dev/ttyUSB0").await,
            Err(RadioError::DeviceUnavailable(_))
        ));
    }
}
