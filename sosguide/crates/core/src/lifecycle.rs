//! État du cycle de vie du nœud : provisioning → urgence.
//!
//! Deux phases, une seule transition possible et irréversible à chaud
//! (cf. CLAUDE.md § Cycle de vie) :
//!
//! - [`Lifecycle::Provisioning`] : premier démarrage, le nœud n'est pas
//!   configuré. Seul le SSID de configuration est diffusé.
//! - [`Lifecycle::Emergency`] : nœud configuré, services d'urgence actifs.
//!
//! Type purement domaine : aucune dépendance d'infrastructure. La décision
//! d'activation revient à la couche application, mais la **règle** de
//! transition vit ici, testable et indépendante des adaptateurs.

/// Erreur de transition du cycle de vie.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LifecycleError {
    /// Activation demandée alors que le nœud n'est pas en provisioning
    /// (il est déjà en urgence : la transition est à sens unique).
    #[error("activation impossible : le nœud n'est pas en phase de provisioning")]
    NotProvisioning,
}

/// Phase du cycle de vie du nœud.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// Premier démarrage, non configuré (SSID `SOS-SETUP-XXXX` uniquement).
    Provisioning,
    /// Configuré : portail, DNS, mesh et liens longue distance actifs.
    Emergency,
}

/// Nom de fil de la phase de provisioning (interop v2.5).
pub const STATE_PROVISIONING: &str = "STATE_PROVISIONING";
/// Nom de fil de la phase d'urgence (interop v2.5).
pub const STATE_EMERGENCY: &str = "STATE_EMERGENCY";

impl Lifecycle {
    /// Déduit la phase de l'indicateur `installed` de la configuration :
    /// un nœud installé est en urgence, sinon en provisioning.
    #[must_use]
    pub fn from_installed(installed: bool) -> Self {
        if installed {
            Self::Emergency
        } else {
            Self::Provisioning
        }
    }

    /// Phase à partir de son nom de fil (interop v2.5), `None` si inconnu.
    #[must_use]
    pub fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            STATE_PROVISIONING => Some(Self::Provisioning),
            STATE_EMERGENCY => Some(Self::Emergency),
            _ => None,
        }
    }

    /// Nom de fil de la phase (interop v2.5).
    #[must_use]
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Provisioning => STATE_PROVISIONING,
            Self::Emergency => STATE_EMERGENCY,
        }
    }

    /// `true` si le nœud est configuré et en phase d'urgence.
    #[must_use]
    pub fn is_emergency(self) -> bool {
        matches!(self, Self::Emergency)
    }

    /// `true` si une activation (provisioning → urgence) est possible.
    #[must_use]
    pub fn can_activate(self) -> bool {
        matches!(self, Self::Provisioning)
    }

    /// Active le nœud : `Provisioning` → `Emergency`. Transition à sens unique :
    /// activer un nœud déjà en urgence est une erreur (jamais un retour arrière
    /// silencieux qui désactiverait les services d'urgence).
    pub fn activate(self) -> Result<Self, LifecycleError> {
        match self {
            Self::Provisioning => Ok(Self::Emergency),
            Self::Emergency => Err(LifecycleError::NotProvisioning),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_flag_maps_to_phase() {
        assert_eq!(Lifecycle::from_installed(false), Lifecycle::Provisioning);
        assert_eq!(Lifecycle::from_installed(true), Lifecycle::Emergency);
    }

    #[test]
    fn activation_is_one_way() -> Result<(), LifecycleError> {
        let node = Lifecycle::Provisioning;
        assert!(node.can_activate());
        let active = node.activate()?;
        assert_eq!(active, Lifecycle::Emergency);
        assert!(active.is_emergency());
        // Réactiver un nœud déjà en urgence est refusé (pas de retour arrière).
        assert_eq!(active.activate(), Err(LifecycleError::NotProvisioning));
        Ok(())
    }

    #[test]
    fn wire_name_roundtrip() {
        for phase in [Lifecycle::Provisioning, Lifecycle::Emergency] {
            assert_eq!(Lifecycle::from_wire_name(phase.wire_name()), Some(phase));
        }
        assert_eq!(Lifecycle::from_wire_name("STATE_UNKNOWN"), None);
    }
}
