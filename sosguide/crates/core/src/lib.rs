//! Domaine métier de SOS-GUIDE : entités, états du cycle de vie, règles.
//! Aucune dépendance d'infrastructure.

pub mod active_alert;
pub mod alert;
pub mod inbox;
pub mod lifecycle;
pub mod official;
pub mod runtime;
pub mod version;

pub use active_alert::{ActiveAlert, MAX_INSTRUCTIONS_CHARS};
pub use alert::{AlertError, AlertPacket, AlertType, PROTOCOL_VERSION};
pub use inbox::{Admission, AlertInbox, InboxAlert};
pub use lifecycle::{Lifecycle, LifecycleError, STATE_EMERGENCY, STATE_PROVISIONING};
pub use official::{OfficialBulletin, OfficialCache, OfficialCategory};
pub use runtime::RuntimeSignal;
pub use version::{sha256_hex, VersionError, VersionManifest};

/// Nom du réseau WiFi de la borne. Constante **partagée** : le portail
/// (QR de l'affiche) et `sos-network` (hostapd) doivent désigner le même SSID —
/// source unique, garantit que le QR imprimé pointe vers le réseau réellement
/// diffusé. *La bascule WPA↔ouvert selon l'alerte est portée par `sos-network`.*
pub const WIFI_SSID: &str = "SOS-GUIDE";
