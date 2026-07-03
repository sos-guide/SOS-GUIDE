//! Persistance locale sur Redb : identité du nœud et configuration.
//!
//! Un seul fichier de base, tolérant aux coupures (transactions ACID de Redb,
//! zéro dépendance externe). Remplace les écritures de fichiers éparses de la
//! v2.5. La **projection publique** de la configuration (sans les secrets) est
//! calculée ici, jamais servie depuis le fichier brut — ce qui corrige la fuite
//! `GET /data/config.json` de la v2.5.

use std::path::{Path, PathBuf};
use std::process::Command;

use redb::{Database, ReadableDatabase, TableDefinition};

/// Identité du nœud (clé de signature) stockée dans `IDENTITY`.
const IDENTITY: TableDefinition<&str, &str> = TableDefinition::new("identity");
/// Configuration du nœud (JSON brut) stockée dans `CONFIG`.
const CONFIG: TableDefinition<&str, &str> = TableDefinition::new("config");
/// Alerte active du nœud (JSON brut) stockée dans `ALERT`.
const ALERT: TableDefinition<&str, &str> = TableDefinition::new("alert");
/// Cache des bulletins officiels (JSON brut) stocké dans `OFFICIAL`.
const OFFICIAL: TableDefinition<&str, &str> = TableDefinition::new("official");
/// Registre des nœuds de confiance (`trusted_nodes.json` v2.5) dans `TRUSTED`.
const TRUSTED: TableDefinition<&str, &str> = TableDefinition::new("trusted");
/// Groupes de ping (JSON array) dans `GROUPS`.
const GROUPS: TableDefinition<&str, &str> = TableDefinition::new("groups");
/// Pings reçus (JSON array) dans `PINGS`.
const PINGS: TableDefinition<&str, &str> = TableDefinition::new("pings");

/// Clé de l'identifiant de nœud dans la table `IDENTITY`.
const KEY_NODE_ID: &str = "node_id";
/// Clé de la clé privée Ed25519 (PEM PKCS#8) dans la table `IDENTITY`.
const KEY_PRIVATE_PEM: &str = "private_key_pem";
/// Clé du document de configuration dans la table `CONFIG`.
const KEY_CONFIG: &str = "json";
/// Clé de l'alerte active dans la table `ALERT`.
const KEY_ACTIVE_ALERT: &str = "active";
/// Clé du cache des bulletins officiels dans la table `OFFICIAL`.
const KEY_OFFICIAL_CACHE: &str = "cache";
/// Clé du registre des nœuds de confiance dans la table `TRUSTED`.
const KEY_TRUSTED_NODES: &str = "nodes";
/// Clé de la liste de groupes de ping dans la table `GROUPS`.
const KEY_GROUPS: &str = "list";
/// Clé de la liste de pings reçus dans la table `PINGS`.
const KEY_PINGS: &str = "list";
/// Clé du sel du mot de passe administrateur dans la table `IDENTITY`.
const KEY_ADMIN_SALT: &str = "admin_salt";
/// Clé de l'empreinte du mot de passe administrateur dans la table `IDENTITY`.
const KEY_ADMIN_HASH: &str = "admin_hash";

/// Champs jamais exposés dans la projection **publique** servie au portail.
/// `authorities` contient des adresses de contact institutionnel (e-mail,
/// `.onion`) : jamais public, mais éditable par l'admin (cf. [`Store::admin_config`]).
/// `wifiPassword` n'est plus produit (AP toujours ouvert depuis 2026-06-28), mais
/// reste dans la liste par **défense en profondeur** : une config héritée v2.5
/// importée ne doit jamais fuiter un secret.
const SECRET_CONFIG_KEYS: &[&str] = &["wifiPassword", "authorities"];
/// Champs masqués dans la projection **administrateur** (l'admin voit tout).
const ADMIN_HIDDEN_CONFIG_KEYS: &[&str] = &[];

/// Erreurs de la couche de persistance.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Ouverture/création de la base impossible.
    #[error("base de données : {0}")]
    Database(#[from] redb::DatabaseError),
    /// Démarrage d'une transaction impossible.
    #[error("transaction : {0}")]
    Transaction(#[from] redb::TransactionError),
    /// Ouverture d'une table impossible.
    #[error("table : {0}")]
    Table(#[from] redb::TableError),
    /// Lecture/écriture dans une table impossible.
    #[error("stockage : {0}")]
    Storage(#[from] redb::StorageError),
    /// Validation d'une transaction impossible.
    #[error("commit : {0}")]
    Commit(#[from] redb::CommitError),
    /// Configuration fournie non décodable en JSON.
    #[error("configuration JSON invalide : {0}")]
    Json(#[from] serde_json::Error),
    /// E/S lors de l'instantané durable (copie working → SOSDATA).
    #[error("instantané durable : {0}")]
    Io(#[from] std::io::Error),
    /// La commande privilégiée de commit durable a échoué.
    #[error("commit durable : {0}")]
    DurableCommit(String),
}

/// Identité persistée du nœud.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredIdentity {
    /// Identifiant du nœud.
    pub node_id: String,
    /// Clé privée Ed25519 au format PEM PKCS#8 (interop v2.5).
    pub private_key_pem: String,
}

/// Copie atomique et durable de `src` vers `dst` : écriture dans un fichier
/// temporaire, `fsync`, `rename` (atomique), puis `fsync` du répertoire pour
/// durabiliser le renommage. Survit à une coupure : `dst` est soit l'ancien
/// contenu intact, soit le nouveau complet — jamais hybride.
fn atomic_copy(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = dst.with_extension("redb.tmp");
    std::fs::copy(src, &tmp)?;
    std::fs::File::open(&tmp)?.sync_all()?;
    std::fs::rename(&tmp, dst)?;
    if let Some(parent) = dst.parent() {
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

/// Cible d'un instantané durable (modèle « SOSDATA en lecture seule »).
///
/// La base de travail (`working`) vit sur un support inscriptible — typiquement
/// un tmpfs (RAM) sur l'appliance Alpine *diskless*. À chaque écriture, son
/// contenu cohérent (post-commit Redb) est recopié vers `target`, sur la
/// partition de données persistante (SOSDATA). Comme SOSDATA est montée en
/// lecture seule, la copie passe par une **commande privilégiée** (`commit_cmd`)
/// qui remonte rw → copie → refsync → remonte ro. Sans commande, la copie est
/// faite en place (support déjà inscriptible : Debian, tests).
struct Durable {
    /// Base de travail (tmpfs/RAM), réellement ouverte par Redb.
    working: PathBuf,
    /// Instantané durable sur SOSDATA, source de vérité au reboot.
    target: PathBuf,
    /// Commande privilégiée `argv` recevant en plus `<working> <target>`.
    /// `None` ⇒ copie atomique en in-process (support inscriptible).
    commit_cmd: Option<Vec<String>>,
}

/// Base de persistance du nœud.
pub struct Store {
    db: Database,
    /// Stratégie d'instantané durable, ou `None` (la base de travail est
    /// elle-même durable — comportement historique).
    durable: Option<Durable>,
}

impl Store {
    /// Ouvre (ou crée) la base au chemin donné et garantit l'existence des
    /// tables, afin que les lectures ultérieures ne rencontrent jamais de
    /// table absente. Le dossier parent doit exister. La base ouverte est
    /// elle-même durable (aucun instantané) : usage historique (support rw).
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let db = Database::create(path)?;
        let txn = db.begin_write()?;
        {
            // Crée les tables si nécessaire (idempotent).
            txn.open_table(IDENTITY)?;
            txn.open_table(CONFIG)?;
            txn.open_table(ALERT)?;
            txn.open_table(OFFICIAL)?;
            txn.open_table(TRUSTED)?;
            txn.open_table(GROUPS)?;
            txn.open_table(PINGS)?;
        }
        txn.commit()?;
        Ok(Self { db, durable: None })
    }

    /// Ouvre la base en mode **instantané durable** (modèle SOSDATA ro).
    ///
    /// `working` est la base réellement ouverte (tmpfs/RAM) ; `target` est
    /// l'instantané durable sur SOSDATA. Au démarrage, si `target` existe et que
    /// `working` est absente, l'instantané est restauré (config/identité/secrets
    /// retrouvés). Après chaque écriture, `working` est recopiée vers `target`
    /// via `commit_cmd` (commande privilégiée remount-rw/copie/remount-ro) ou,
    /// si `None`, par copie atomique en place.
    pub fn open_durable(
        working: impl AsRef<Path>,
        target: impl AsRef<Path>,
        commit_cmd: Option<String>,
    ) -> Result<Self, StoreError> {
        let working = working.as_ref().to_path_buf();
        let target = target.as_ref().to_path_buf();
        // Amorçage : restaurer l'instantané durable dans la base de travail.
        if target.exists() && !working.exists() {
            if let Some(parent) = working.parent() {
                std::fs::create_dir_all(parent)?;
            }
            atomic_copy(&target, &working)?;
        }
        let mut store = Self::open(&working)?;
        let commit_cmd = commit_cmd
            .map(|c| c.split_whitespace().map(str::to_owned).collect::<Vec<_>>())
            .filter(|v| !v.is_empty());
        store.durable = Some(Durable {
            working,
            target,
            commit_cmd,
        });
        Ok(store)
    }

    /// Recopie la base de travail vers l'instantané durable. No-op si la base
    /// est déjà durable. Appelé après **chaque** transaction d'écriture validée
    /// (le fichier Redb est alors cohérent sur disque).
    fn persist_durable(&self) -> Result<(), StoreError> {
        let Some(d) = &self.durable else {
            return Ok(());
        };
        match &d.commit_cmd {
            Some(argv) => {
                // argv non vide (filtré à l'ouverture).
                let (prog, args) = argv.split_first().ok_or_else(|| {
                    StoreError::DurableCommit("commande de commit vide".to_owned())
                })?;
                let status = Command::new(prog)
                    .args(args)
                    .arg(&d.working)
                    .arg(&d.target)
                    .status()?;
                if !status.success() {
                    return Err(StoreError::DurableCommit(format!(
                        "{prog} a échoué (code {:?})",
                        status.code()
                    )));
                }
                Ok(())
            }
            None => atomic_copy(&d.working, &d.target).map_err(StoreError::from),
        }
    }

    /// Enregistre (ou remplace) l'identité du nœud.
    pub fn save_identity(&self, node_id: &str, private_key_pem: &str) -> Result<(), StoreError> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(IDENTITY)?;
            table.insert(KEY_NODE_ID, node_id)?;
            table.insert(KEY_PRIVATE_PEM, private_key_pem)?;
        }
        txn.commit()?;
        self.persist_durable()?;
        Ok(())
    }

    /// Charge l'identité du nœud, ou `None` si aucune n'est encore stockée.
    pub fn load_identity(&self) -> Result<Option<StoredIdentity>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(IDENTITY)?;
        let node_id = table.get(KEY_NODE_ID)?.map(|v| v.value().to_owned());
        let private_key_pem = table.get(KEY_PRIVATE_PEM)?.map(|v| v.value().to_owned());
        match (node_id, private_key_pem) {
            (Some(node_id), Some(private_key_pem)) => Ok(Some(StoredIdentity {
                node_id,
                private_key_pem,
            })),
            _ => Ok(None),
        }
    }

    /// Enregistre la configuration (validée comme JSON avant écriture).
    pub fn save_config(&self, json: &str) -> Result<(), StoreError> {
        // Refuse un document non-JSON : la projection publique en dépend.
        let _: serde_json::Value = serde_json::from_str(json)?;
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(CONFIG)?;
            table.insert(KEY_CONFIG, json)?;
        }
        txn.commit()?;
        self.persist_durable()?;
        Ok(())
    }

    /// Charge la configuration brute (complète, secrets inclus) — usage interne.
    pub fn load_config(&self) -> Result<Option<String>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(CONFIG)?;
        Ok(table.get(KEY_CONFIG)?.map(|v| v.value().to_owned()))
    }

    /// Projette la configuration en retirant `hidden_keys`. Renvoie `None` si
    /// aucune configuration n'est encore persistée.
    fn project_config(&self, hidden_keys: &[&str]) -> Result<Option<String>, StoreError> {
        let Some(raw) = self.load_config()? else {
            return Ok(None);
        };
        let mut value: serde_json::Value = serde_json::from_str(&raw)?;
        if let Some(object) = value.as_object_mut() {
            for key in hidden_keys {
                object.remove(*key);
            }
        }
        Ok(Some(serde_json::to_string(&value)?))
    }

    /// Projection **publique** de la configuration : sans aucun champ sensible
    /// ([`SECRET_CONFIG_KEYS`]). C'est la seule forme servable au portail public.
    pub fn public_config(&self) -> Result<Option<String>, StoreError> {
        self.project_config(SECRET_CONFIG_KEYS)
    }

    /// Projection **administrateur** : tout sauf les secrets à rotation dédiée
    /// ([`ADMIN_HIDDEN_CONFIG_KEYS`]). Sert le formulaire `/admin` (où l'admin
    /// édite p. ex. les destinataires `authorities`, masqués au public).
    pub fn admin_config(&self) -> Result<Option<String>, StoreError> {
        self.project_config(ADMIN_HIDDEN_CONFIG_KEYS)
    }

    /// `true` si la configuration persistée marque le nœud comme installé
    /// (champ `"installed": true`). Sert à déduire la phase du cycle de vie.
    /// `false` si aucune configuration ou si le champ est absent.
    pub fn config_installed(&self) -> Result<bool, StoreError> {
        let Some(raw) = self.load_config()? else {
            return Ok(false);
        };
        let value: serde_json::Value = serde_json::from_str(&raw)?;
        Ok(value
            .get("installed")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false))
    }

    /// Enregistre (ou remplace) l'alerte active (validée comme JSON).
    pub fn save_active_alert(&self, json: &str) -> Result<(), StoreError> {
        let _: serde_json::Value = serde_json::from_str(json)?;
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(ALERT)?;
            table.insert(KEY_ACTIVE_ALERT, json)?;
        }
        txn.commit()?;
        self.persist_durable()?;
        Ok(())
    }

    /// Charge l'alerte active (JSON brut), ou `None` si aucune n'est en cours.
    pub fn load_active_alert(&self) -> Result<Option<String>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(ALERT)?;
        Ok(table.get(KEY_ACTIVE_ALERT)?.map(|v| v.value().to_owned()))
    }

    /// Efface l'alerte active (retour à la normale). Idempotent.
    pub fn clear_active_alert(&self) -> Result<(), StoreError> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(ALERT)?;
            table.remove(KEY_ACTIVE_ALERT)?;
        }
        txn.commit()?;
        self.persist_durable()?;
        Ok(())
    }

    /// Enregistre (ou remplace) le cache des bulletins officiels (validé JSON).
    pub fn save_official(&self, json: &str) -> Result<(), StoreError> {
        let _: serde_json::Value = serde_json::from_str(json)?;
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(OFFICIAL)?;
            table.insert(KEY_OFFICIAL_CACHE, json)?;
        }
        txn.commit()?;
        self.persist_durable()?;
        Ok(())
    }

    /// Charge le cache des bulletins officiels (JSON brut), ou `None` si vide.
    pub fn load_official(&self) -> Result<Option<String>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(OFFICIAL)?;
        Ok(table.get(KEY_OFFICIAL_CACHE)?.map(|v| v.value().to_owned()))
    }

    /// Vide le cache des bulletins officiels. Idempotent.
    pub fn clear_official(&self) -> Result<(), StoreError> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(OFFICIAL)?;
            table.remove(KEY_OFFICIAL_CACHE)?;
        }
        txn.commit()?;
        self.persist_durable()?;
        Ok(())
    }

    /// Enregistre (ou remplace) le registre des nœuds de confiance (validé JSON).
    pub fn save_trusted(&self, json: &str) -> Result<(), StoreError> {
        let _: serde_json::Value = serde_json::from_str(json)?;
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(TRUSTED)?;
            table.insert(KEY_TRUSTED_NODES, json)?;
        }
        txn.commit()?;
        self.persist_durable()?;
        Ok(())
    }

    /// Charge le registre des nœuds de confiance (JSON brut), ou `None` si vide.
    pub fn load_trusted(&self) -> Result<Option<String>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(TRUSTED)?;
        Ok(table.get(KEY_TRUSTED_NODES)?.map(|v| v.value().to_owned()))
    }

    /// Vide le registre des nœuds de confiance. Idempotent.
    pub fn clear_trusted(&self) -> Result<(), StoreError> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(TRUSTED)?;
            table.remove(KEY_TRUSTED_NODES)?;
        }
        txn.commit()?;
        self.persist_durable()?;
        Ok(())
    }

    /// Enregistre (ou remplace) l'empreinte du mot de passe administrateur.
    pub fn save_admin_password(&self, salt_hex: &str, hash_hex: &str) -> Result<(), StoreError> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(IDENTITY)?;
            table.insert(KEY_ADMIN_SALT, salt_hex)?;
            table.insert(KEY_ADMIN_HASH, hash_hex)?;
        }
        txn.commit()?;
        self.persist_durable()?;
        Ok(())
    }

    /// **Retour aux valeurs d'usine** : efface la configuration, l'alerte active,
    /// le cache officiel et le mot de passe administrateur — le nœud repasse en
    /// provisioning (`/install`). L'**identité cryptographique** du nœud
    /// (identifiant et clé privée Ed25519) est **conservée** — ce n'est pas un
    /// secret opérationnel mais l'identité matérielle de la borne. Idempotent.
    pub fn factory_reset(&self) -> Result<(), StoreError> {
        let txn = self.db.begin_write()?;
        {
            txn.open_table(CONFIG)?.remove(KEY_CONFIG)?;
            txn.open_table(ALERT)?.remove(KEY_ACTIVE_ALERT)?;
            txn.open_table(OFFICIAL)?.remove(KEY_OFFICIAL_CACHE)?;
            txn.open_table(TRUSTED)?.remove(KEY_TRUSTED_NODES)?;
            txn.open_table(GROUPS)?.remove(KEY_GROUPS)?;
            txn.open_table(PINGS)?.remove(KEY_PINGS)?;
            let mut identity = txn.open_table(IDENTITY)?;
            identity.remove(KEY_ADMIN_SALT)?;
            identity.remove(KEY_ADMIN_HASH)?;
        }
        txn.commit()?;
        self.persist_durable()?;
        Ok(())
    }

    /// Charge l'empreinte du mot de passe administrateur `(sel, empreinte)`,
    /// ou `None` si aucun mot de passe n'a encore été défini.
    pub fn load_admin_password(&self) -> Result<Option<(String, String)>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(IDENTITY)?;
        let salt = table.get(KEY_ADMIN_SALT)?.map(|v| v.value().to_owned());
        let hash = table.get(KEY_ADMIN_HASH)?.map(|v| v.value().to_owned());
        match (salt, hash) {
            (Some(salt), Some(hash)) => Ok(Some((salt, hash))),
            _ => Ok(None),
        }
    }

    /// Enregistre (ou remplace) la liste des groupes de ping (validée JSON).
    pub fn save_groups(&self, json: &str) -> Result<(), StoreError> {
        let _: serde_json::Value = serde_json::from_str(json)?;
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(GROUPS)?;
            table.insert(KEY_GROUPS, json)?;
        }
        txn.commit()?;
        self.persist_durable()?;
        Ok(())
    }

    /// Charge la liste des groupes de ping (JSON brut), ou `None` si vide.
    pub fn load_groups(&self) -> Result<Option<String>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(GROUPS)?;
        Ok(table.get(KEY_GROUPS)?.map(|v| v.value().to_owned()))
    }

    /// Enregistre (ou remplace) la liste des pings reçus (validée JSON).
    pub fn save_pings(&self, json: &str) -> Result<(), StoreError> {
        let _: serde_json::Value = serde_json::from_str(json)?;
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(PINGS)?;
            table.insert(KEY_PINGS, json)?;
        }
        txn.commit()?;
        self.persist_durable()?;
        Ok(())
    }

    /// Charge la liste des pings reçus (JSON brut), ou `None` si vide.
    pub fn load_pings(&self) -> Result<Option<String>, StoreError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(PINGS)?;
        Ok(table.get(KEY_PINGS)?.map(|v| v.value().to_owned()))
    }

    /// Vide la liste des pings reçus. Idempotent.
    pub fn clear_pings(&self) -> Result<(), StoreError> {
        let txn = self.db.begin_write()?;
        {
            let mut table = txn.open_table(PINGS)?;
            table.remove(KEY_PINGS)?;
        }
        txn.commit()?;
        self.persist_durable()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// Chemin de base temporaire unique (pas de dépendance externe).
    fn temp_db() -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "sos-store-test-{}-{nanos}.redb",
            std::process::id()
        ))
    }

    #[test]
    fn identity_roundtrip_and_persists_across_reopen() -> TestResult {
        let path = temp_db();
        {
            let store = Store::open(&path)?;
            assert_eq!(store.load_identity()?, None);
            store.save_identity(
                "node-A",
                "-----BEGIN PRIVATE KEY-----\nx\n-----END PRIVATE KEY-----",
            )?;
        }
        // Réouverture : l'identité survit (tolérance aux coupures/redémarrages).
        let store = Store::open(&path)?;
        let id = store
            .load_identity()?
            .ok_or("identité absente après réouverture")?;
        assert_eq!(id.node_id, "node-A");
        assert!(id.private_key_pem.contains("BEGIN PRIVATE KEY"));
        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    /// Modèle SOSDATA ro : la base de travail (tmpfs) est recopiée vers
    /// l'instantané durable ; après un « reboot » (perte de la base de travail)
    /// l'état est restauré sans reconfiguration.
    #[test]
    fn durable_snapshot_restores_state_after_simulated_reboot() -> TestResult {
        let working = temp_db();
        let target = temp_db();
        // 1er boot : pas d'instantané → base neuve ; écritures recopiées en place.
        {
            let store = Store::open_durable(&working, &target, None)?;
            store.save_identity(
                "node-Z",
                "-----BEGIN PRIVATE KEY-----\nz\n-----END PRIVATE KEY-----",
            )?;
            store.save_config(r#"{"establishment":{"name":"École"},"installed":true}"#)?;
        }
        assert!(target.exists(), "instantané durable créé sur SOSDATA");
        // Reboot diskless : la base de travail (tmpfs/RAM) disparaît.
        std::fs::remove_file(&working)?;
        // 2e boot : restauration depuis l'instantané.
        let store = Store::open_durable(&working, &target, None)?;
        let id = store.load_identity()?.ok_or("identité perdue au reboot")?;
        assert_eq!(id.node_id, "node-Z");
        assert!(store.config_installed()?, "config restaurée");
        let _ = std::fs::remove_file(&working);
        let _ = std::fs::remove_file(&target);
        Ok(())
    }

    /// La commande de commit externe (ici `cp <working> <target>`) produit bien
    /// l'instantané — exerce le seam de la commande privilégiée (remount helper).
    #[test]
    fn durable_commit_cmd_external_copies_snapshot() -> TestResult {
        let working = temp_db();
        let target = temp_db();
        {
            let store = Store::open_durable(&working, &target, Some("cp".to_owned()))?;
            store.save_config(r#"{"installed":true}"#)?;
        }
        assert!(target.exists(), "cp a produit l'instantané");
        std::fs::remove_file(&working)?;
        let store = Store::open_durable(&working, &target, Some("cp".to_owned()))?;
        assert!(store.config_installed()?);
        let _ = std::fs::remove_file(&working);
        let _ = std::fs::remove_file(&target);
        Ok(())
    }

    /// Une commande de commit qui échoue (`false`) remonte une erreur explicite
    /// — l'admin sait que l'écriture durable n'a pas abouti.
    #[test]
    fn durable_commit_cmd_failure_is_reported() -> TestResult {
        let working = temp_db();
        let target = temp_db();
        let store = Store::open_durable(&working, &target, Some("false".to_owned()))?;
        let res = store.save_config(r#"{"installed":true}"#);
        assert!(
            matches!(res, Err(StoreError::DurableCommit(_))),
            "le commit durable aurait dû échouer avec DurableCommit"
        );
        let _ = std::fs::remove_file(&working);
        Ok(())
    }

    #[test]
    fn factory_reset_wipes_operational_data_keeps_identity() -> TestResult {
        let path = temp_db();
        let store = Store::open(&path)?;
        store.save_identity(
            "node-A",
            "-----BEGIN PRIVATE KEY-----\nx\n-----END PRIVATE KEY-----",
        )?;
        store.save_config(r#"{"establishment":{"name":"Mairie"},"installed":true}"#)?;
        store.save_admin_password("salt", "hash")?;
        store.save_active_alert(r#"{"active":true}"#)?;
        store.save_official(r#"{"bulletins":[]}"#)?;

        store.factory_reset()?;

        // Données opérationnelles effacées → nœud en provisioning.
        assert_eq!(store.load_config()?, None);
        assert!(!store.config_installed()?);
        assert_eq!(store.load_admin_password()?, None);
        assert_eq!(store.load_active_alert()?, None);
        assert_eq!(store.load_official()?, None);
        // Identité matérielle conservée.
        assert!(store.load_identity()?.is_some());
        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    #[test]
    fn public_config_strips_secrets() -> TestResult {
        let path = temp_db();
        let store = Store::open(&path)?;
        store.save_config(r#"{"establishment":{"name":"Mairie"},"wifiPassword":"secret"}"#)?;
        let public = store.public_config()?.ok_or("config absente")?;
        assert!(!public.contains("wifiPassword"));
        assert!(public.contains("Mairie"));
        // La forme brute conserve le secret (usage interne uniquement).
        let raw = store.load_config()?.ok_or("config brute absente")?;
        assert!(raw.contains("secret"));
        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    #[test]
    fn authorities_hidden_from_public_but_visible_to_admin() -> TestResult {
        let path = temp_db();
        let store = Store::open(&path)?;
        store.save_config(
            r#"{"establishment":{"name":"Mairie"},"wifiPassword":"secret","authorities":[{"name":"Pompiers","address":"x@y.fr"}]}"#,
        )?;
        // Public : ni le WiFi ni les destinataires institutionnels.
        let public = store.public_config()?.ok_or("config publique absente")?;
        assert!(!public.contains("wifiPassword"));
        assert!(!public.contains("authorities"));
        assert!(!public.contains("Pompiers"));
        // Admin : destinataires visibles (édition) ET le mot de passe WiFi
        // (l'admin doit pouvoir le lire pour l'imprimer sur l'affiche).
        let admin = store.admin_config()?.ok_or("config admin absente")?;
        assert!(admin.contains("Pompiers"));
        assert!(admin.contains("wifiPassword"));
        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    #[test]
    fn save_config_rejects_invalid_json() -> TestResult {
        let path = temp_db();
        let store = Store::open(&path)?;
        assert!(store.save_config("pas du json").is_err());
        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    #[test]
    fn admin_password_roundtrip() -> TestResult {
        let path = temp_db();
        let store = Store::open(&path)?;
        assert_eq!(store.load_admin_password()?, None);
        store.save_admin_password("ab12", "cd34")?;
        assert_eq!(
            store.load_admin_password()?,
            Some(("ab12".to_owned(), "cd34".to_owned()))
        );
        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    #[test]
    fn active_alert_save_load_clear() -> TestResult {
        let path = temp_db();
        let store = Store::open(&path)?;
        assert_eq!(store.load_active_alert()?, None);
        store.save_active_alert(r#"{"cause":"INCENDIE","instructions":"Évacuez","since":1}"#)?;
        let loaded = store.load_active_alert()?.ok_or("alerte absente")?;
        assert!(loaded.contains("INCENDIE"));
        // Effacement idempotent : un second appel ne doit pas échouer.
        store.clear_active_alert()?;
        store.clear_active_alert()?;
        assert_eq!(store.load_active_alert()?, None);
        // JSON invalide refusé (la projection en dépend).
        assert!(store.save_active_alert("pas du json").is_err());
        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    #[test]
    fn official_cache_save_load_clear() -> TestResult {
        let path = temp_db();
        let store = Store::open(&path)?;
        assert_eq!(store.load_official()?, None);
        store.save_official(r#"{"bulletins":[{"source":"VigiCrues","category":"WEATHER","country":"fr","title":"Crue","body":"x","published":1,"fetched":2}],"updated":2}"#)?;
        let loaded = store.load_official()?.ok_or("cache absent")?;
        assert!(loaded.contains("VigiCrues"));
        store.clear_official()?;
        store.clear_official()?;
        assert_eq!(store.load_official()?, None);
        assert!(store.save_official("pas du json").is_err());
        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    #[test]
    fn config_installed_reflects_flag() -> TestResult {
        let path = temp_db();
        let store = Store::open(&path)?;
        // Aucune config → non installé (phase provisioning).
        assert!(!store.config_installed()?);
        store.save_config(r#"{"installed": false}"#)?;
        assert!(!store.config_installed()?);
        store.save_config(r#"{"installed": true}"#)?;
        assert!(store.config_installed()?);
        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    #[test]
    fn trusted_nodes_save_load_clear() -> TestResult {
        let path = temp_db();
        let store = Store::open(&path)?;
        assert_eq!(store.load_trusted()?, None);
        let json = r#"{"nodes":{"voisin":{"public_key":"-----BEGIN PUBLIC KEY-----\nX\n-----END PUBLIC KEY-----\n"}}}"#;
        store.save_trusted(json)?;
        assert_eq!(store.load_trusted()?.as_deref(), Some(json));
        store.clear_trusted()?;
        assert_eq!(store.load_trusted()?, None);
        let _ = std::fs::remove_file(&path);
        Ok(())
    }

    #[test]
    fn factory_reset_clears_trusted_nodes() -> TestResult {
        let path = temp_db();
        let store = Store::open(&path)?;
        store.save_trusted(r#"{"nodes":{}}"#)?;
        store.factory_reset()?;
        assert_eq!(store.load_trusted()?, None);
        let _ = std::fs::remove_file(&path);
        Ok(())
    }
}
