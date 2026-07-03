//! Manifeste public du nœud — **seule** donnée exposée sur le service caché Tor.
//!
//! Surface `.onion` volontairement **minuscule et sûre** (cf. CLAUDE.md
//! § `sos-gateway`) : identification du nœud + état d'alerte, **jamais** le
//! portail, **jamais** l'administration, **jamais** la configuration complète.
//! Le constructeur est **pur** et testé ; ce qu'il ne met pas dans le manifeste
//! ne peut pas fuiter.

use sos_core::RuntimeSignal;

/// Identifiant de service, constant : permet à un pair distant de reconnaître un
/// nœud SOS-GUIDE derrière une adresse `.onion`.
const SERVICE_TAG: &str = "sos-guide";

/// Construit le manifeste public à partir de l'identité (statique) et du signal
/// runtime (phase + alerte). Aucun secret, aucune projection de configuration.
#[must_use]
pub fn build(node_id: &str, version: &str, signal: RuntimeSignal) -> serde_json::Value {
    serde_json::json!({
        "service": SERVICE_TAG,
        "nodeId": node_id,
        "version": version,
        "phase": signal.phase().wire_name(),
        "alertActive": signal.alert_active,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn manifest_identifies_node_and_alert() -> TestResult {
        let signal = RuntimeSignal {
            installed: true,
            alert_active: true,
        };
        let m = build("ecole-a", "0.1.0", signal);
        assert_eq!(m.get("service").and_then(|v| v.as_str()), Some("sos-guide"));
        assert_eq!(m.get("nodeId").and_then(|v| v.as_str()), Some("ecole-a"));
        assert_eq!(m.get("version").and_then(|v| v.as_str()), Some("0.1.0"));
        assert_eq!(
            m.get("phase").and_then(|v| v.as_str()),
            Some("STATE_EMERGENCY")
        );
        assert_eq!(
            m.get("alertActive").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        Ok(())
    }

    #[test]
    fn provisioning_phase_is_reflected() {
        let m = build("n", "0.1.0", RuntimeSignal::default());
        assert_eq!(
            m.get("phase").and_then(|v| v.as_str()),
            Some("STATE_PROVISIONING")
        );
        assert_eq!(
            m.get("alertActive").and_then(serde_json::Value::as_bool),
            Some(false)
        );
    }

    #[test]
    fn manifest_leaks_no_secret_fields() {
        // Garde-fou : la surface .onion ne doit jamais porter de secret ni de config.
        let m = build("n", "0.1.0", RuntimeSignal::default());
        for forbidden in [
            "wifiPassword",
            "authorities",
            "config",
            "adminPassword",
            "privateKey",
            "establishment",
        ] {
            assert!(
                m.get(forbidden).is_none(),
                "champ interdit exposé : {forbidden}"
            );
        }
    }
}
