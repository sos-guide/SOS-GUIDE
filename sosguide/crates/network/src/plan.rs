//! Décision du mode point d'accès, **pure** et testable.
//!
//! Règle produit (cf. CLAUDE.md § Modèle d'accès réseau) : **l'AP est TOUJOURS
//! OUVERT** — un kiosque d'urgence public ne doit dresser aucun obstacle devant
//! un citoyen (décision 2026-06-28). Le seul choix restant est le **SSID** :
//! - **Provisioning** : SSID de configuration `SOS-SETUP-XXXX`.
//! - **Emergency** : SSID public d'urgence `SOS-GUIDE`.

use sos_core::{Lifecycle, RuntimeSignal};

/// Rôle du réseau diffusé.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApKind {
    /// SSID de configuration (premier démarrage).
    Setup,
    /// SSID public d'urgence.
    Public,
}

/// Plan d'AP à appliquer : quel SSID, quel rôle. Toujours **ouvert**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApPlan {
    /// Rôle du réseau (configuration vs public).
    pub kind: ApKind,
    /// SSID à diffuser.
    pub ssid: String,
}

/// Calcule le plan d'AP à partir du signal runtime.
///
/// `setup_ssid` = SSID de configuration (premier démarrage) ; `public_ssid` =
/// SSID public d'urgence (constante partagée). Le réseau est toujours ouvert.
#[must_use]
pub fn plan_for(signal: RuntimeSignal, setup_ssid: &str, public_ssid: &str) -> ApPlan {
    match signal.phase() {
        Lifecycle::Provisioning => ApPlan {
            kind: ApKind::Setup,
            ssid: setup_ssid.to_owned(),
        },
        Lifecycle::Emergency => ApPlan {
            kind: ApKind::Public,
            ssid: public_ssid.to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETUP: &str = "SOS-SETUP-AB12CD34";
    const PUBLIC: &str = "SOS-GUIDE";

    fn sig(installed: bool, alert: bool) -> RuntimeSignal {
        RuntimeSignal {
            installed,
            alert_active: alert,
        }
    }

    #[test]
    fn provisioning_broadcasts_setup_ssid() {
        let plan = plan_for(sig(false, false), SETUP, PUBLIC);
        assert_eq!(plan.kind, ApKind::Setup);
        assert_eq!(plan.ssid, SETUP);
    }

    #[test]
    fn emergency_broadcasts_public_ssid() {
        // Avec ou sans alerte : SSID public, toujours ouvert.
        for alert in [false, true] {
            let plan = plan_for(sig(true, alert), SETUP, PUBLIC);
            assert_eq!(plan.kind, ApKind::Public, "alert={alert}");
            assert_eq!(plan.ssid, PUBLIC);
        }
    }
}
