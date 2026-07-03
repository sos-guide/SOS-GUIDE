//! Mise à jour OTA « pull » du binaire applicatif (modèle diskless A/B).
//!
//! Flux : lit la configuration (URL + activation) sur la FAT, télécharge le
//! manifeste signé **puis** le binaire via le `curl` **système** (jamais de
//! client HTTPS Rust : `ring`/asm C casserait le binaire statique musl), vérifie
//! l'empreinte SHA-256 + la signature Ed25519 (clé de publication **épinglée**
//! dans l'image), **refuse tout downgrade**, puis délègue l'installation dans le
//! slot FAT et le reboot au helper privilégié `sos-apply-update`.
//!
//! Anti-bricage : le binaire d'usine reste dans l'apkovl ; le slot FAT n'est
//! qu'une **surcouche OTA** sélectionnée au boot si elle vérifie et est plus
//! récente, sinon on retombe sur l'usine.

use std::path::{Path, PathBuf};
use std::process::Command;

use sos_core::VersionManifest;

/// Configuration de mise à jour (fichier `update.conf` sur la partition FAT,
/// éditable sans recompiler — comme `wifi.conf`).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct UpdateConfig {
    /// URL de base servant `manifest.json` et `sos-guide.bin` (HTTP/Tor).
    pub url: String,
    /// Mise à jour automatique active (`ENABLED=1`).
    pub enabled: bool,
    /// Options `curl` additionnelles (ex. `--socks5-hostname 127.0.0.1:9050`
    /// pour une URL `.onion`).
    pub curl_opts: Vec<String>,
}

impl UpdateConfig {
    /// Analyse `update.conf` : lignes `CLE=valeur`, `#` commentaire, CRLF toléré.
    /// Clés reconnues : `URL`, `ENABLED` (1/true/oui/on), `CURL_OPTS`.
    pub fn parse(text: &str) -> Self {
        let mut cfg = UpdateConfig::default();
        for raw in text.lines() {
            let line = raw.trim_end_matches('\r').trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            match k.trim() {
                "URL" => cfg.url = v.trim().to_owned(),
                "ENABLED" => {
                    cfg.enabled = matches!(v.trim(), "1" | "true" | "TRUE" | "oui" | "yes" | "on")
                }
                "CURL_OPTS" => cfg.curl_opts = v.split_whitespace().map(str::to_owned).collect(),
                _ => {}
            }
        }
        cfg
    }
}

/// Compare deux versions « x.y.z » numériquement (repli sur 0 par segment).
/// `true` si `a` est **strictement** supérieure à `b` (cœur de l'anti-downgrade).
pub fn version_gt(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split(['.', '-', '+'])
            .map(|p| {
                p.chars()
                    .take_while(char::is_ascii_digit)
                    .collect::<String>()
            })
            .map(|d| d.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let (va, vb) = (parse(a), parse(b));
    for i in 0..va.len().max(vb.len()) {
        let x = va.get(i).copied().unwrap_or(0);
        let y = vb.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

/// Vérifie empreinte SHA-256 **et** signature Ed25519 d'un manifeste contre un
/// binaire et une clé publique PEM. `Err(message)` explicite sinon.
pub fn verify_manifest(
    manifest: &VersionManifest,
    binary: &[u8],
    pubkey_pem: &str,
) -> Result<(), String> {
    if !manifest.matches_binary(binary) {
        return Err("empreinte SHA-256 du binaire ≠ manifeste".to_owned());
    }
    let Some(sig) = manifest.signature.as_deref() else {
        return Err("manifeste non signé".to_owned());
    };
    sos_security::verify_detached(pubkey_pem, &manifest.canonical_payload(), sig)
        .map_err(|e| format!("signature invalide: {e}"))
}

/// Version installée = la plus haute entre le manifeste d'usine (apkovl) et
/// celui du slot OTA. Repli `0.0.0` si aucun n'est lisible.
pub fn current_version(factory_manifest: &Path, slot_manifest: &Path) -> String {
    let read = |p: &Path| {
        std::fs::read_to_string(p)
            .ok()
            .and_then(|s| VersionManifest::from_json(&s).ok())
            .map(|m| m.version)
    };
    let factory = read(factory_manifest).unwrap_or_else(|| "0.0.0".to_owned());
    match read(slot_manifest) {
        Some(slot) if version_gt(&slot, &factory) => slot,
        _ => factory,
    }
}

/// Chemins de la mise à jour (overridables pour les tests).
#[derive(Debug, Clone)]
pub struct Paths {
    /// `update.conf` (FAT).
    pub config: PathBuf,
    /// Clé publique de publication épinglée (apkovl).
    pub pubkey: PathBuf,
    /// Manifeste d'usine (apkovl).
    pub factory_manifest: PathBuf,
    /// Manifeste du slot OTA (FAT).
    pub slot_manifest: PathBuf,
    /// Helper privilégié qui installe dans le slot + reboote.
    pub apply_helper: PathBuf,
    /// Répertoire de travail (tmpfs).
    pub tmp_dir: PathBuf,
}

impl Paths {
    /// Emplacements de production sur le nœud diskless.
    pub fn default_prod() -> Self {
        Paths {
            config: PathBuf::from("/media/mmcblk0p1/update.conf"),
            pubkey: PathBuf::from("/etc/sosguide/release.pub"),
            factory_manifest: PathBuf::from("/etc/sosguide/manifest.json"),
            slot_manifest: PathBuf::from("/media/mmcblk0p1/manifest.json"),
            apply_helper: PathBuf::from("/usr/local/sbin/sos-apply-update"),
            tmp_dir: PathBuf::from("/run/sos-update"),
        }
    }
}

/// Issue d'une exécution de mise à jour.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Désactivée (`ENABLED=0` ou URL absente).
    Disabled,
    /// Aucune version plus récente disponible.
    UpToDate,
    /// Nouvelle version installée (le helper a déclenché le reboot).
    Applied(String),
}

/// Exécute le cycle complet de mise à jour pull. Tout échec (réseau, signature,
/// downgrade) est non destructif : on conserve l'état courant.
pub fn run(paths: &Paths) -> Result<Outcome, String> {
    let cfg = UpdateConfig::parse(&std::fs::read_to_string(&paths.config).unwrap_or_default());
    if !cfg.enabled || cfg.url.is_empty() {
        return Ok(Outcome::Disabled);
    }
    let pubkey = std::fs::read_to_string(&paths.pubkey)
        .map_err(|e| format!("clé publique illisible ({}): {e}", paths.pubkey.display()))?;

    std::fs::create_dir_all(&paths.tmp_dir)
        .map_err(|e| format!("répertoire de travail ({}): {e}", paths.tmp_dir.display()))?;
    let man_tmp = paths.tmp_dir.join("manifest.json");
    let bin_tmp = paths.tmp_dir.join("sos-guide.bin");

    // 1. Manifeste distant (petit, récupéré d'abord pour décider).
    curl(&cfg, &join_url(&cfg.url, "manifest.json"), &man_tmp)?;
    let remote = std::fs::read_to_string(&man_tmp)
        .map_err(|e| format!("manifeste distant illisible: {e}"))
        .and_then(|raw| {
            VersionManifest::from_json(&raw).map_err(|e| format!("manifeste invalide: {e}"))
        })?;

    // 2. Anti-downgrade : ignorer une version ≤ courante (sans rien télécharger).
    let current = current_version(&paths.factory_manifest, &paths.slot_manifest);
    if !version_gt(&remote.version, &current) {
        return Ok(Outcome::UpToDate);
    }

    // 3. Binaire distant, puis vérification empreinte + signature.
    curl(&cfg, &join_url(&cfg.url, "sos-guide.bin"), &bin_tmp)?;
    let binary = std::fs::read(&bin_tmp).map_err(|e| format!("binaire distant illisible: {e}"))?;
    verify_manifest(&remote, &binary, &pubkey)?;

    // 4. Installation privilégiée (slot FAT A/B + reboot) déléguée au helper.
    let status = Command::new(&paths.apply_helper)
        .arg(&bin_tmp)
        .arg(&man_tmp)
        .status()
        .map_err(|e| format!("helper d'installation indisponible: {e}"))?;
    if !status.success() {
        return Err("installation (sos-apply-update) en échec".to_owned());
    }
    Ok(Outcome::Applied(remote.version))
}

/// Télécharge `url` vers `dest` via le `curl` système (échec = `Err`).
fn curl(cfg: &UpdateConfig, url: &str, dest: &Path) -> Result<(), String> {
    let status = Command::new("curl")
        .arg("-fsS")
        .arg("--max-time")
        .arg("120")
        .args(&cfg.curl_opts)
        .arg("-o")
        .arg(dest)
        .arg(url)
        .status()
        .map_err(|e| format!("curl indisponible: {e}"))?;
    if !status.success() {
        return Err(format!("téléchargement échoué: {url}"));
    }
    Ok(())
}

/// Concatène une base d'URL et un nom de fichier (slash unique).
fn join_url(base: &str, file: &str) -> String {
    if base.ends_with('/') {
        format!("{base}{file}")
    } else {
        format!("{base}/{file}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_gt_compares_numeric_segments() {
        assert!(version_gt("0.2.0", "0.1.9"));
        assert!(version_gt("1.0.0", "0.9.9"));
        assert!(version_gt("0.1.10", "0.1.9"));
        assert!(!version_gt("0.1.0", "0.1.0"));
        assert!(!version_gt("0.1.0", "0.2.0"));
        // Repli sur 0 pour les segments manquants.
        assert!(version_gt("1.1", "1.0.9"));
        assert!(!version_gt("1.0", "1.0.0"));
        // Suffixes non numériques ignorés (pré-release traitée comme le socle).
        assert!(version_gt("0.2.0-rc1", "0.1.0"));
    }

    #[test]
    fn config_parse_reads_keys_and_tolerates_crlf() {
        let cfg = UpdateConfig::parse(
            "# commentaire\r\nURL = http://u/sos\r\nENABLED=1\nCURL_OPTS=--socks5-hostname 127.0.0.1:9050\n",
        );
        assert_eq!(cfg.url, "http://u/sos");
        assert!(cfg.enabled);
        assert_eq!(cfg.curl_opts, vec!["--socks5-hostname", "127.0.0.1:9050"]);
    }

    #[test]
    fn config_disabled_by_default_and_when_zero() {
        assert!(!UpdateConfig::parse("URL=http://u").enabled);
        assert!(!UpdateConfig::parse("ENABLED=0\nURL=http://u").enabled);
        assert!(UpdateConfig::parse("ENABLED=oui").enabled);
    }

    #[test]
    fn join_url_single_slash() {
        assert_eq!(join_url("http://u/d", "m.json"), "http://u/d/m.json");
        assert_eq!(join_url("http://u/d/", "m.json"), "http://u/d/m.json");
    }
}
