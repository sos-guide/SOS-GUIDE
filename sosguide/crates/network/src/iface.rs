//! Génération des commandes de configuration de l'interface de l'AP.
//!
//! Avant de lancer `hostapd`, l'interface doit porter l'IP de la passerelle
//! (`10.0.0.1/24`), être active, et l'IPv6 doit être désactivée (le nœud est
//! IPv4-only : aucune autoconfiguration, aucune fuite d'adresse). [`iface_commands`]
//! est **pure** (produit des listes d'arguments) et testée ; l'exécution réelle
//! (`ip`/`sysctl` via `tokio::process`) n'a lieu qu'en mode `live`.

/// Adresse de la passerelle/AP (cf. plan Phase 3).
pub const GATEWAY_IP: &str = "10.0.0.1";
/// Préfixe CIDR du réseau de l'AP.
pub const GATEWAY_CIDR: &str = "10.0.0.1/24";

/// Une commande système à exécuter : exécutable + arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// Programme à lancer (ex. `ip`, `sysctl`).
    pub program: String,
    /// Arguments passés au programme.
    pub args: Vec<String>,
}

impl Command {
    fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_owned(),
            args: args.iter().map(|s| (*s).to_owned()).collect(),
        }
    }
}

/// Construit la séquence de commandes amenant `iface` à l'état requis pour l'AP :
/// purge des adresses, attribution de `cidr`, activation, IPv6 désactivée.
#[must_use]
pub fn iface_commands(iface: &str, cidr: &str) -> Vec<Command> {
    let disable_ipv6 = format!("net.ipv6.conf.{iface}.disable_ipv6=1");
    vec![
        // Repart d'un état propre : aucune IP héritée.
        Command::new("ip", &["addr", "flush", "dev", iface]),
        Command::new("ip", &["addr", "add", cidr, "dev", iface]),
        Command::new("ip", &["link", "set", iface, "up"]),
        // IPv6 off : île IPv4 stricte, pas d'autoconfiguration.
        Command::new("sysctl", &["-w", disable_ipv6.as_str()]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn assigns_gateway_cidr_and_brings_up() -> TestResult {
        let cmds = iface_commands("wlan0", GATEWAY_CIDR);
        let add = cmds
            .iter()
            .find(|c| {
                c.args.first().map(String::as_str) == Some("addr")
                    && c.args.contains(&"add".to_owned())
            })
            .ok_or("pas de commande addr add")?;
        assert!(add.args.contains(&GATEWAY_CIDR.to_owned()));
        assert!(add.args.contains(&"wlan0".to_owned()));

        let up = cmds
            .iter()
            .any(|c| c.args == ["link", "set", "wlan0", "up"]);
        assert!(up);
        Ok(())
    }

    #[test]
    fn flush_runs_before_add() -> TestResult {
        let cmds = iface_commands("wlan0", GATEWAY_CIDR);
        let flush = cmds
            .iter()
            .position(|c| c.args.contains(&"flush".to_owned()));
        let add = cmds.iter().position(|c| c.args.contains(&"add".to_owned()));
        assert!(flush < add, "flush ({flush:?}) doit précéder add ({add:?})");
        Ok(())
    }

    #[test]
    fn disables_ipv6_on_the_interface() {
        let cmds = iface_commands("wlan0", GATEWAY_CIDR);
        let sysctl = cmds.iter().any(|c| {
            c.program == "sysctl"
                && c.args
                    .iter()
                    .any(|a| a == "net.ipv6.conf.wlan0.disable_ipv6=1")
        });
        assert!(sysctl);
    }
}
