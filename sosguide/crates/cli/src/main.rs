//! `sos-cli` — outils d'administration du nœud SOS-GUIDE.
//!
//! Sous-commandes :
//! - `health [chemin]` : imprime les vitaux du nœud en JSON (défaut `/`) ;
//! - `watchdog [device] [secs] [sonde]` : caresse le chien de garde matériel
//!   tant que le démon répond (défaut `/dev/watchdog`, `15` s, `127.0.0.1:8080`) ;
//! - `sign-update <binaire> <version> <clé-privée.pem> [date]` : produit un
//!   manifeste de version signé (JSON sur stdout) — côté publication ;
//! - `verify-update <manifeste.json> <binaire> <clé-publique.pem>` : vérifie
//!   l'empreinte et la signature ; code de sortie 0 si valide, 1 sinon ;
//! - `update` : mise à jour OTA « pull » (télécharge + vérifie + anti-downgrade,
//!   installe dans le slot FAT et reboote) — déclenchée par `crond`.
//!
//! Sans argument : `health`.

use std::net::{SocketAddr, TcpStream};
use std::process::ExitCode;
use std::time::Duration;

use sos_cli::{health, watchdog};

/// Délai de connexion de la sonde applicative du watchdog.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("health");
    match command {
        "health" => cmd_health(args.get(1).map(String::as_str).unwrap_or("/")),
        "watchdog" => cmd_watchdog(&args),
        "sign-update" => cmd_sign_update(&args),
        "verify-update" => cmd_verify_update(&args),
        "update" => cmd_update(),
        other => {
            eprintln!(
                "sos-cli: commande inconnue « {other} » (health | watchdog | sign-update | verify-update | update)"
            );
            ExitCode::FAILURE
        }
    }
}

/// Imprime les vitaux en JSON. Toujours un succès : des vitaux partiels valent
/// mieux que pas de sortie (un champ indisponible vaut `null`).
fn cmd_health(disk_path: &str) -> ExitCode {
    let vitals = health::collect(disk_path);
    match serde_json::to_string_pretty(&vitals.to_json()) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("sos-cli: sérialisation des vitaux impossible: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Lance la boucle du chien de garde. La sonde applicative vérifie que le démon
/// accepte une connexion TCP sur son adresse d'écoute.
fn cmd_watchdog(args: &[String]) -> ExitCode {
    init_tracing();
    let device = args.get(1).map(String::as_str).unwrap_or("/dev/watchdog");
    let interval = args
        .get(2)
        .and_then(|s| s.parse::<u64>().ok())
        .map_or(Duration::from_secs(15), Duration::from_secs);
    let probe: SocketAddr = args
        .get(3)
        .map(String::as_str)
        .unwrap_or("127.0.0.1:8080")
        .parse()
        .unwrap_or_else(|_| ([127, 0, 0, 1], 8080).into());

    let healthy = move || TcpStream::connect_timeout(&probe, PROBE_TIMEOUT).is_ok();
    match watchdog::run(device, interval, healthy) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("sos-cli: watchdog indisponible ({device}): {err}");
            ExitCode::FAILURE
        }
    }
}

/// Produit un manifeste de version signé : `sign-update <binaire> <version>
/// <clé-privée.pem> [date]`. Le JSON signé est imprimé sur stdout.
fn cmd_sign_update(args: &[String]) -> ExitCode {
    let (Some(bin_path), Some(version), Some(key_path)) = (args.get(1), args.get(2), args.get(3))
    else {
        eprintln!("usage: sos-cli sign-update <binaire> <version> <clé-privée.pem> [date]");
        return ExitCode::FAILURE;
    };
    let built_at = args.get(4).cloned().unwrap_or_default();
    let binary = match std::fs::read(bin_path) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("sos-cli: lecture du binaire impossible ({bin_path}): {err}");
            return ExitCode::FAILURE;
        }
    };
    let key_pem = match std::fs::read_to_string(key_path) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("sos-cli: lecture de la clé impossible ({key_path}): {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut manifest = sos_core::VersionManifest::for_binary(version, &built_at, &binary);
    match sos_security::sign_detached(&key_pem, &manifest.canonical_payload()) {
        Ok(sig) => manifest.signature = Some(sig),
        Err(err) => {
            eprintln!("sos-cli: signature impossible: {err}");
            return ExitCode::FAILURE;
        }
    }
    match manifest.to_json() {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("sos-cli: sérialisation du manifeste impossible: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Vérifie un manifeste signé : `verify-update <manifeste.json> <binaire>
/// <clé-publique.pem>`. Code de sortie 0 si empreinte + signature valides.
fn cmd_verify_update(args: &[String]) -> ExitCode {
    let (Some(man_path), Some(bin_path), Some(key_path)) = (args.get(1), args.get(2), args.get(3))
    else {
        eprintln!("usage: sos-cli verify-update <manifeste.json> <binaire> <clé-publique.pem>");
        return ExitCode::FAILURE;
    };
    let manifest = match std::fs::read_to_string(man_path)
        .map_err(|e| e.to_string())
        .and_then(|raw| sos_core::VersionManifest::from_json(&raw).map_err(|e| e.to_string()))
    {
        Ok(m) => m,
        Err(err) => {
            eprintln!("sos-cli: manifeste illisible ({man_path}): {err}");
            return ExitCode::FAILURE;
        }
    };
    let binary = match std::fs::read(bin_path) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("sos-cli: lecture du binaire impossible ({bin_path}): {err}");
            return ExitCode::FAILURE;
        }
    };
    if !manifest.matches_binary(&binary) {
        eprintln!("❌ empreinte SHA-256 du binaire ≠ manifeste — binaire altéré ou erroné");
        return ExitCode::FAILURE;
    }
    let Some(signature) = manifest.signature.as_deref() else {
        eprintln!("❌ manifeste non signé");
        return ExitCode::FAILURE;
    };
    let key_pem = match std::fs::read_to_string(key_path) {
        Ok(p) => p,
        Err(err) => {
            eprintln!("sos-cli: lecture de la clé publique impossible ({key_path}): {err}");
            return ExitCode::FAILURE;
        }
    };
    match sos_security::verify_detached(&key_pem, &manifest.canonical_payload(), signature) {
        Ok(()) => {
            println!(
                "✅ version {} vérifiée (empreinte + signature)",
                manifest.version
            );
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("❌ signature invalide: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Mise à jour OTA « pull » : `update` (sans argument). Lit `update.conf`,
/// télécharge + vérifie (signature + empreinte) + anti-downgrade, puis délègue
/// l'installation dans le slot FAT et le reboot au helper `sos-apply-update`.
/// Pensée pour un déclenchement périodique par `crond` (modèle flotte autonome).
fn cmd_update() -> ExitCode {
    init_tracing();
    let paths = sos_cli::update::Paths::default_prod();
    match sos_cli::update::run(&paths) {
        Ok(sos_cli::update::Outcome::Disabled) => {
            tracing::info!("mise à jour désactivée (update.conf : ENABLED/URL)");
            ExitCode::SUCCESS
        }
        Ok(sos_cli::update::Outcome::UpToDate) => {
            tracing::info!("nœud déjà à jour");
            ExitCode::SUCCESS
        }
        Ok(sos_cli::update::Outcome::Applied(version)) => {
            tracing::info!(%version, "mise à jour installée — reboot pour appliquer");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("sos-cli update: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Journalisation minimale (la sortie part dans journald sous systemd).
fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
}
