//! Diffusion de la transaction par le **nœud-sortie** (celui qui a Internet).
//!
//! Comme les tuiles OSM, on passe par le **`curl` système** plutôt qu'un client
//! HTTPS Rust : aucune toolchain C ni TLS embarqué ne doit casser le binaire
//! statique `aarch64-musl`. On poste la transaction brute (hex) au corps d'une
//! requête `POST` vers l'API publique de diffusion.
//!
//! Module **pur** : on construit les arguments `curl` ; leur exécution vit dans
//! l'orchestrateur `live` (différé, cf. [`crate`]). La borne ne fait que **relayer
//! et diffuser** — jamais de garde de clés ni de fonds.

/// Endpoint « POST transaction brute » par défaut (mempool.space). L'opérateur
/// peut le remplacer (Blockstream `https://blockstream.info/api/tx`, un mempool
/// auto-hébergé, etc.) via [`crate::PayConfig`].
pub const DEFAULT_BROADCAST_API: &str = "https://mempool.space/api/tx";

/// Construit les arguments `curl` pour diffuser une transaction (hex brut en corps).
///
/// `-fsS` : échoue proprement sur code HTTP d'erreur, silencieux sauf en cas
/// d'erreur (adapté à un service). Le corps est la transaction hexadécimale ;
/// l'API renvoie le txid réseau en cas de succès.
#[must_use]
pub fn broadcast_argv(api_url: &str, raw_hex: &str) -> Vec<String> {
    vec![
        "-fsS".to_owned(),
        "-X".to_owned(),
        "POST".to_owned(),
        "--data".to_owned(),
        raw_hex.to_owned(),
        api_url.to_owned(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_posts_raw_hex_to_url() {
        let argv = broadcast_argv(DEFAULT_BROADCAST_API, "0100ab");
        assert_eq!(
            argv,
            vec![
                "-fsS".to_owned(),
                "-X".to_owned(),
                "POST".to_owned(),
                "--data".to_owned(),
                "0100ab".to_owned(),
                "https://mempool.space/api/tx".to_owned(),
            ]
        );
        // L'URL est toujours le dernier argument (cible), la charge juste avant.
        assert_eq!(argv.last().map(String::as_str), Some(DEFAULT_BROADCAST_API));
    }
}
