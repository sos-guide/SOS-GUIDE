//! Signal d'état runtime du nœud, partagé entre l'interface (portail) et
//! l'infrastructure réseau (`sos-network`) pour piloter le point d'accès à
//! chaud — sans coupler les deux couches (toutes deux dépendent du domaine).
//!
//! Le portail est la source de vérité (il écrit la config et l'alerte dans
//! Redb) ; il publie ce signal à chaque changement. Le réseau s'y abonne et en
//! déduit le mode du point d'accès : configuration, protégé en veille, ouvert
//! en alerte (cf. CLAUDE.md § Cycle de vie).

use crate::Lifecycle;

/// Instantané de l'état runtime gouvernant le réseau local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuntimeSignal {
    /// `true` si le nœud est installé (phase urgence), sinon provisioning.
    pub installed: bool,
    /// `true` si une alerte est active : l'AP public doit alors s'ouvrir
    /// (aucune barrière pour un citoyen en détresse).
    pub alert_active: bool,
}

impl RuntimeSignal {
    /// Phase du cycle de vie déduite de l'indicateur `installed`.
    #[must_use]
    pub fn phase(self) -> Lifecycle {
        Lifecycle::from_installed(self.installed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_provisioning_no_alert() {
        let s = RuntimeSignal::default();
        assert_eq!(s.phase(), Lifecycle::Provisioning);
        assert!(!s.alert_active);
    }

    #[test]
    fn installed_maps_to_emergency() {
        let s = RuntimeSignal {
            installed: true,
            alert_active: true,
        };
        assert_eq!(s.phase(), Lifecycle::Emergency);
    }
}
