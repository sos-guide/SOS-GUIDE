//! Chien de garde matériel (`/dev/watchdog`, ex. `bcm2835_wdt` du Pi).
//!
//! À l'ouverture du périphérique, le noyau **arme** le watchdog : si on cesse de
//! le « caresser » (keepalive) avant l'expiration, la carte **redémarre** — la
//! garantie de reprise quand le démon est figé. La fermeture propre écrit l'octet
//! magique `V` pour **désarmer** (pas de redémarrage à l'arrêt volontaire).
//!
//! [`run`] ne caresse le watchdog que tant qu'une **sonde applicative** le juge
//! sain : si l'application est figée, le keepalive s'arrête et le matériel reprend
//! la main. Aucune panique : un périphérique absent est signalé par une erreur.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::time::Duration;

/// Octet magique de fermeture propre : désarme le watchdog (RFC du pilote Linux).
const MAGIC_CLOSE: &[u8] = b"V";
/// Octet de keepalive (toute écriture ≠ `V` caresse le chien).
const KEEPALIVE: &[u8] = b"\0";

/// Poignée sur le périphérique watchdog. **Armé tant qu'il vit** ; désarmé au
/// `Drop` (écriture de l'octet magique).
pub struct Watchdog {
    device: File,
}

impl Watchdog {
    /// Ouvre (et **arme**) le watchdog. Échoue proprement si le périphérique est
    /// absent (noyau sans `CONFIG_WATCHDOG`, ou hôte non concerné).
    pub fn open(path: &str) -> std::io::Result<Self> {
        let device = OpenOptions::new().write(true).open(path)?;
        Ok(Self { device })
    }

    /// Caresse le watchdog : repousse l'échéance de redémarrage.
    pub fn pet(&mut self) -> std::io::Result<()> {
        self.device.write_all(KEEPALIVE)
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        // Fermeture propre : désarme pour ne pas redémarrer à l'arrêt volontaire.
        let _ = self.device.write_all(MAGIC_CLOSE);
        let _ = self.device.flush();
    }
}

/// Boucle de surveillance : ouvre le watchdog puis le caresse toutes les
/// `interval`, **uniquement** tant que `healthy()` est vrai. Dès que la sonde
/// applicative échoue, on cesse de caresser : le matériel redémarrera la carte.
///
/// Retourne une erreur si le périphérique ne peut pas être ouvert ; sinon ne
/// retourne pas (boucle de service) jusqu'à ce que `healthy()` reste faux assez
/// longtemps pour déclencher le redémarrage matériel.
pub fn run(path: &str, interval: Duration, healthy: impl Fn() -> bool) -> std::io::Result<()> {
    let mut dog = Watchdog::open(path)?;
    tracing::info!(path, secs = interval.as_secs(), "watchdog armé");
    loop {
        if healthy() {
            if let Err(err) = dog.pet() {
                tracing::warn!(%err, "watchdog: keepalive impossible");
            }
        } else {
            tracing::error!("watchdog: sonde applicative en échec — keepalive suspendu");
        }
        std::thread::sleep(interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_missing_device_errors_without_panic() {
        // Aucun périphérique à ce chemin : erreur propre, jamais de panique.
        assert!(Watchdog::open("/definitely/not/a/watchdog/device").is_err());
    }
}
