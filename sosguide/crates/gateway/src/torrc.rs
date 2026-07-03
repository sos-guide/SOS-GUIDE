//! Génération de la configuration `tor` du service caché v3.
//!
//! `tor` est un démon **externe** (hérité v2.5) : on ne le réimplémente pas, on
//! génère son `torrc`. Le service caché publie **uniquement** le port du
//! manifeste local (jamais le portail ni l'admin), et le client SOCKS est
//! désactivé (`SocksPort 0`) — cette surface n'émet pas, elle n'expose que le
//! manifeste. La génération est **pure** et testée ; l'écriture et le lancement
//! de `tor` ne sont atteints qu'en mode `live` (cf. [`crate::GatewayMode`]).

use std::net::SocketAddr;

/// Port virtuel exposé par le service caché (HTTP standard côté `.onion`).
const ONION_VIRTUAL_PORT: u16 = 80;

/// Génère le contenu d'un `torrc` minimal pour le service caché v3.
///
/// `hs_dir` = répertoire d'état du service caché (clé `.onion` v3) ; `manifest`
/// = adresse locale où le démon redirige les requêtes `.onion` (le serveur de
/// manifeste, lié au loopback).
#[must_use]
pub fn torrc(hs_dir: &str, manifest: SocketAddr) -> String {
    let mut conf = String::new();
    // Pas de relais, pas de sortie : ce nœud n'est qu'un service caché.
    conf.push_str("SocksPort 0\n");
    conf.push_str(&format!("HiddenServiceDir {hs_dir}\n"));
    conf.push_str("HiddenServiceVersion 3\n");
    // La requête .onion:80 est redirigée vers le manifeste local (loopback).
    conf.push_str(&format!(
        "HiddenServicePort {ONION_VIRTUAL_PORT} {manifest}\n"
    ));
    conf
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn sample() -> String {
        torrc(
            "/var/lib/tor/sos-guide",
            SocketAddr::from((Ipv4Addr::LOCALHOST, 9099)),
        )
    }

    #[test]
    fn declares_v3_hidden_service_to_loopback_manifest() {
        let conf = sample();
        assert!(conf.contains("HiddenServiceVersion 3\n"));
        assert!(conf.contains("HiddenServiceDir /var/lib/tor/sos-guide\n"));
        assert!(conf.contains("HiddenServicePort 80 127.0.0.1:9099\n"));
    }

    #[test]
    fn socks_is_disabled() {
        // Surface restreinte : aucun client SOCKS, le nœud n'émet pas par ici.
        assert!(sample().contains("SocksPort 0\n"));
    }
}
