//! Génération des règles `iptables` d'isolation du nœud.
//!
//! Le nœud est une **île** : il ne route rien (`FORWARD DROP`), limite le débit
//! par client (anti-DoS, ~30 req/s par IP), et bloque DNS-over-TLS (port 853)
//! pour forcer la résolution par le DNS local du portail captif. L'IPv6 est
//! désactivée par ailleurs ([`crate::iface`]).
//!
//! [`iptables_rules`] est **pure** (produit des listes d'arguments) et testée ;
//! l'exécution réelle d'`iptables` n'a lieu qu'en mode `live`.

/// Débit maximal de nouvelles requêtes par IP source (req/s) — cf. CLAUDE.md.
const HASHLIMIT_RATE: &str = "30/sec";
/// Taille de la rafale tolérée avant application du débit moyen.
const HASHLIMIT_BURST: &str = "60";
/// Port DNS-over-TLS à bloquer (force le DNS local du portail captif).
const DOT_PORT: &str = "853";

/// Paramètres d'isolation netfilter.
#[derive(Debug, Clone)]
pub struct FwParams {
    /// Interface de l'AP (ex. `wlan0`) à laquelle s'appliquent les limites.
    pub iface: String,
}

/// Construit la liste ordonnée des invocations `iptables` (sans l'exécutable),
/// chaque sous-liste étant les arguments d'une commande.
#[must_use]
pub fn iptables_rules(params: &FwParams) -> Vec<Vec<String>> {
    let iface = params.iface.as_str();
    let arg = |s: &str| s.to_owned();
    vec![
        // Le nœud ne route jamais : aucune redirection de trafic.
        vec![arg("-P"), arg("FORWARD"), arg("DROP")],
        // Boucle locale toujours autorisée.
        vec![
            arg("-A"),
            arg("INPUT"),
            arg("-i"),
            arg("lo"),
            arg("-j"),
            arg("ACCEPT"),
        ],
        // Réponses aux connexions déjà établies.
        vec![
            arg("-A"),
            arg("INPUT"),
            arg("-m"),
            arg("conntrack"),
            arg("--ctstate"),
            arg("ESTABLISHED,RELATED"),
            arg("-j"),
            arg("ACCEPT"),
        ],
        // Limite de débit par IP source sur l'interface de l'AP (anti-DoS).
        vec![
            arg("-A"),
            arg("INPUT"),
            arg("-i"),
            iface.to_owned(),
            arg("-m"),
            arg("hashlimit"),
            arg("--hashlimit-name"),
            arg("sosguide"),
            arg("--hashlimit-mode"),
            arg("srcip"),
            arg("--hashlimit-upto"),
            arg(HASHLIMIT_RATE),
            arg("--hashlimit-burst"),
            arg(HASHLIMIT_BURST),
            arg("-j"),
            arg("ACCEPT"),
        ],
        // Au-delà de la limite, sur l'interface de l'AP : on jette.
        vec![
            arg("-A"),
            arg("INPUT"),
            arg("-i"),
            iface.to_owned(),
            arg("-j"),
            arg("DROP"),
        ],
        // Bloque DNS-over-TLS (TCP/853) : tout passe par le DNS local.
        vec![
            arg("-A"),
            arg("INPUT"),
            arg("-p"),
            arg("tcp"),
            arg("--dport"),
            arg(DOT_PORT),
            arg("-j"),
            arg("REJECT"),
        ],
        // Bloque DoT/QUIC éventuel (UDP/853).
        vec![
            arg("-A"),
            arg("INPUT"),
            arg("-p"),
            arg("udp"),
            arg("--dport"),
            arg(DOT_PORT),
            arg("-j"),
            arg("REJECT"),
        ],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn lines() -> Vec<String> {
        iptables_rules(&FwParams {
            iface: "wlan0".to_owned(),
        })
        .iter()
        .map(|r| r.join(" "))
        .collect()
    }

    #[test]
    fn forward_is_dropped_first() -> TestResult {
        let first = lines().first().cloned().ok_or("aucune règle")?;
        assert_eq!(first, "-P FORWARD DROP");
        Ok(())
    }

    #[test]
    fn hashlimit_uses_srcip_and_rate() -> TestResult {
        let line = lines()
            .into_iter()
            .find(|l| l.contains("hashlimit"))
            .ok_or("pas de règle hashlimit")?;
        assert!(line.contains("--hashlimit-mode srcip"));
        assert!(line.contains("--hashlimit-upto 30/sec"));
        Ok(())
    }

    #[test]
    fn dot_port_is_rejected_tcp_and_udp() {
        let dot: Vec<String> = lines().into_iter().filter(|l| l.contains("853")).collect();
        assert_eq!(dot.len(), 2);
        assert!(dot.iter().any(|l| l.contains("-p tcp")));
        assert!(dot.iter().any(|l| l.contains("-p udp")));
    }
}
