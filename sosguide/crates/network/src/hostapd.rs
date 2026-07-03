//! Génération de la configuration `hostapd` à partir d'un [`ApPlan`].
//!
//! `hostapd` (pilote `nl80211`) est un démon **externe** hérité de la v2.5 : on
//! ne le réimplémente pas, on génère sa configuration et on le (re)démarre. La
//! génération (`hostapd_conf`) est **pure** et testée ; l'écriture du fichier et
//! le redémarrage du service ne sont atteints qu'en mode `live`
//! (cf. [`crate::NetworkMode`]).

use crate::plan::ApPlan;

/// Bande 2,4 GHz (`hw_mode=g`) : portée maximale, compatibilité universelle —
/// on privilégie la couverture en situation d'urgence, pas le débit.
const HW_MODE: &str = "g";

/// Génère le contenu d'un fichier `hostapd.conf` pour le plan donné.
///
/// `iface` = interface radio (ex. `wlan0`) ; `channel` = canal 2,4 GHz ;
/// `country` = code pays ISO 3166-1 (ex. `CH`) pour la conformité réglementaire
/// des fréquences/puissances.
#[must_use]
pub fn hostapd_conf(plan: &ApPlan, iface: &str, channel: u8, country: &str) -> String {
    let mut conf = String::new();
    conf.push_str(&format!("interface={iface}\n"));
    conf.push_str("driver=nl80211\n");
    conf.push_str(&format!("ssid={}\n", plan.ssid));
    conf.push_str(&format!("hw_mode={HW_MODE}\n"));
    conf.push_str(&format!("channel={channel}\n"));
    conf.push_str(&format!("country_code={country}\n"));
    // Respect des contraintes réglementaires locales sur les canaux/puissances.
    conf.push_str("ieee80211d=1\n");
    // Réseau visible : l'AP d'urgence ne se cache jamais.
    conf.push_str("ignore_broadcast_ssid=0\n");
    // Réseau toujours ouvert (aucune ligne WPA) : aucun obstacle pour un citoyen
    // en détresse (cf. CLAUDE.md § Modèle d'accès réseau, décision 2026-06-28).
    conf
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::ApKind;

    fn open_plan() -> ApPlan {
        ApPlan {
            kind: ApKind::Public,
            ssid: "SOS-GUIDE".to_owned(),
        }
    }

    #[test]
    fn conf_is_always_open_and_has_no_wpa_lines() {
        let conf = hostapd_conf(&open_plan(), "wlan0", 6, "CH");
        assert!(conf.contains("interface=wlan0\n"));
        assert!(conf.contains("driver=nl80211\n"));
        assert!(conf.contains("ssid=SOS-GUIDE\n"));
        assert!(conf.contains("channel=6\n"));
        assert!(conf.contains("country_code=CH\n"));
        // Réseau ouvert : jamais de ligne WPA.
        assert!(!conf.contains("wpa"));
    }

    #[test]
    fn ssid_follows_the_plan() {
        let mut plan = open_plan();
        plan.ssid = "SOS-SETUP-AB12CD34".to_owned();
        let conf = hostapd_conf(&plan, "wlan0", 11, "CH");
        assert!(conf.contains("ssid=SOS-SETUP-AB12CD34\n"));
    }
}
