//! Sonde de santé du nœud : température, mémoire, charge, disque.
//!
//! Les vitaux proviennent de `/sys` et `/proc` (lecture de fichiers, pur Rust) ;
//! le disque est lu via `df` (présent partout, comme `curl` pour les tuiles).
//! L'**analyse** (`parse_*`) est séparée de l'**acquisition** (`collect`) : les
//! parseurs sont purs et entièrement testés sur des échantillons réels, sans
//! dépendre du système hôte. Tout vital indisponible vaut `None` (jamais de
//! panique) : un nœud doit pouvoir rendre ses vitaux même partiels.

use std::process::Command;

/// Fichier de température du SoC (millidegrés Celsius).
const THERMAL: &str = "/sys/class/thermal/thermal_zone0/temp";
/// Statistiques mémoire du noyau.
const MEMINFO: &str = "/proc/meminfo";
/// Moyennes de charge du système.
const LOADAVG: &str = "/proc/loadavg";

/// Vitaux du nœud. Chaque champ est optionnel : une source illisible n'empêche
/// pas de rapporter les autres.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Vitals {
    /// Température du SoC en °C.
    pub temp_c: Option<f32>,
    /// Mémoire totale (kio).
    pub mem_total_kb: Option<u64>,
    /// Mémoire disponible (kio).
    pub mem_available_kb: Option<u64>,
    /// Charge moyenne sur 1, 5 et 15 minutes.
    pub load: Option<(f64, f64, f64)>,
    /// Espace disque total de la racine (kio).
    pub disk_total_kb: Option<u64>,
    /// Espace disque disponible de la racine (kio).
    pub disk_available_kb: Option<u64>,
}

/// Convertit la température brute (`/sys`, millidegrés) en °C.
#[must_use]
pub fn parse_thermal(content: &str) -> Option<f32> {
    let millis: i32 = content.trim().parse().ok()?;
    Some(millis as f32 / 1000.0)
}

/// Lit une valeur `MemXxx:` (en kio) de `/proc/meminfo`.
fn meminfo_field(content: &str, prefix: &str) -> Option<u64> {
    content
        .lines()
        .find_map(|line| line.strip_prefix(prefix))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
}

/// Extrait (mémoire totale, mémoire disponible) de `/proc/meminfo`.
#[must_use]
pub fn parse_meminfo(content: &str) -> (Option<u64>, Option<u64>) {
    (
        meminfo_field(content, "MemTotal:"),
        meminfo_field(content, "MemAvailable:"),
    )
}

/// Extrait les moyennes de charge (1, 5, 15 min) de `/proc/loadavg`.
#[must_use]
pub fn parse_loadavg(content: &str) -> Option<(f64, f64, f64)> {
    let mut it = content.split_whitespace();
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next()?.parse().ok()?;
    Some((a, b, c))
}

/// Extrait (total, disponible) en kio de la sortie de `df -kP <chemin>`.
/// La première ligne est l'en-tête ; on lit la première ligne de données.
#[must_use]
pub fn parse_df(content: &str) -> (Option<u64>, Option<u64>) {
    let Some(data) = content.lines().nth(1) else {
        return (None, None);
    };
    let fields: Vec<&str> = data.split_whitespace().collect();
    let total = fields.get(1).and_then(|n| n.parse().ok());
    let available = fields.get(3).and_then(|n| n.parse().ok());
    (total, available)
}

/// Lit la sortie de `df -kP <path>` (best-effort).
fn read_df(path: &str) -> Option<String> {
    let output = Command::new("df").args(["-kP", path]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// Collecte les vitaux du nœud. `disk_path` est le point de montage à mesurer
/// (typiquement `/`). Toute source illisible laisse le champ à `None`.
#[must_use]
pub fn collect(disk_path: &str) -> Vitals {
    let temp_c = std::fs::read_to_string(THERMAL)
        .ok()
        .and_then(|c| parse_thermal(&c));
    let (mem_total_kb, mem_available_kb) = std::fs::read_to_string(MEMINFO)
        .map(|c| parse_meminfo(&c))
        .unwrap_or((None, None));
    let load = std::fs::read_to_string(LOADAVG)
        .ok()
        .and_then(|c| parse_loadavg(&c));
    let (disk_total_kb, disk_available_kb) = read_df(disk_path)
        .map(|c| parse_df(&c))
        .unwrap_or((None, None));
    Vitals {
        temp_c,
        mem_total_kb,
        mem_available_kb,
        load,
        disk_total_kb,
        disk_available_kb,
    }
}

impl Vitals {
    /// Projette les vitaux en JSON (champs absents → `null`). Inclut des dérivés
    /// utiles (pourcentages d'usage mémoire/disque) quand calculables.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        let mem_used_pct = used_pct(self.mem_total_kb, self.mem_available_kb);
        let disk_used_pct = used_pct(self.disk_total_kb, self.disk_available_kb);
        let (l1, l5, l15) = match self.load {
            Some((a, b, c)) => (Some(a), Some(b), Some(c)),
            None => (None, None, None),
        };
        serde_json::json!({
            "tempC": self.temp_c,
            "memTotalKb": self.mem_total_kb,
            "memAvailableKb": self.mem_available_kb,
            "memUsedPct": mem_used_pct,
            "load1": l1,
            "load5": l5,
            "load15": l15,
            "diskTotalKb": self.disk_total_kb,
            "diskAvailableKb": self.disk_available_kb,
            "diskUsedPct": disk_used_pct,
        })
    }
}

/// Pourcentage utilisé = (total − dispo) / total · 100, arrondi à l'entier.
fn used_pct(total: Option<u64>, available: Option<u64>) -> Option<u64> {
    let total = total?;
    let available = available?;
    if total == 0 || available > total {
        return None;
    }
    Some((total - available).saturating_mul(100) / total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thermal_millidegrees_to_celsius() {
        assert_eq!(parse_thermal("52124\n"), Some(52.124));
        assert_eq!(parse_thermal("pas un nombre"), None);
    }

    #[test]
    fn meminfo_extracts_total_and_available() {
        let sample = "MemTotal:        2028032 kB\nMemFree:          100000 kB\nMemAvailable:    1500000 kB\n";
        assert_eq!(parse_meminfo(sample), (Some(2_028_032), Some(1_500_000)));
    }

    #[test]
    fn meminfo_missing_field_is_none() {
        assert_eq!(parse_meminfo("MemFree: 1 kB\n"), (None, None));
    }

    #[test]
    fn loadavg_reads_three_averages() {
        assert_eq!(
            parse_loadavg("0.12 0.34 0.45 1/234 5678"),
            Some((0.12, 0.34, 0.45))
        );
        assert_eq!(parse_loadavg("0.1 0.2"), None);
    }

    #[test]
    fn df_reads_total_and_available() {
        let sample = "Filesystem     1024-blocks    Used Available Capacity Mounted on\n/dev/root         30000000 5000000  24000000      18% /\n";
        assert_eq!(parse_df(sample), (Some(30_000_000), Some(24_000_000)));
    }

    #[test]
    fn df_without_data_line_is_none() {
        assert_eq!(
            parse_df("Filesystem 1024-blocks Used Available\n"),
            (None, None)
        );
    }

    #[test]
    fn used_pct_is_computed_and_guarded() {
        assert_eq!(used_pct(Some(100), Some(25)), Some(75));
        assert_eq!(used_pct(Some(0), Some(0)), None);
        assert_eq!(used_pct(Some(100), Some(200)), None); // dispo > total : incohérent
        assert_eq!(used_pct(None, Some(1)), None);
    }

    #[test]
    fn json_carries_derived_percentages() {
        let vitals = Vitals {
            temp_c: Some(50.0),
            mem_total_kb: Some(1000),
            mem_available_kb: Some(250),
            load: Some((0.5, 0.4, 0.3)),
            disk_total_kb: Some(2000),
            disk_available_kb: Some(500),
        };
        let json = vitals.to_json();
        assert_eq!(
            json.get("memUsedPct").and_then(serde_json::Value::as_u64),
            Some(75)
        );
        assert_eq!(
            json.get("diskUsedPct").and_then(serde_json::Value::as_u64),
            Some(75)
        );
        assert_eq!(
            json.get("load1").and_then(serde_json::Value::as_f64),
            Some(0.5)
        );
    }
}
