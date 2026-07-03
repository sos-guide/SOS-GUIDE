//! Bulletins officiels mis en cache pour l'affichage hors-ligne.
//!
//! Une alerte combine deux contenus : les **consignes locales** saisies par
//! l'admin ([`crate::ActiveAlert`]) et des **bulletins officiels** (gouvernement,
//! météo, catastrophes naturelles, consignes nationales) — récupérés quand le
//! nœud a une connectivité, puis **mis en cache** pour rester consultables une
//! fois la coupure survenue. Ce module définit le modèle de contenu et le cache ;
//! il est purement domaine (aucune I/O, aucun transport réseau).
//!
//! L'origine d'un bulletin — récupération automatique sur un canal de sortie
//! (Ethernet de maintenance / Tor, Phases ultérieures) **ou** import manuel par
//! l'admin (repli toujours disponible, sans réseau) — est indifférente ici : les
//! deux produisent le même [`OfficialBulletin`] ingéré par
//! [`OfficialCache::ingest`].

use serde::{Deserialize, Serialize};

use crate::alert::truncate_chars;

/// Longueur maximale du nom de source (caractères).
pub const MAX_SOURCE_CHARS: usize = 120;
/// Longueur maximale du titre (caractères).
pub const MAX_TITLE_CHARS: usize = 200;
/// Longueur maximale du corps (caractères).
pub const MAX_BODY_CHARS: usize = 4000;
/// Longueur maximale du code pays (caractères).
pub const MAX_COUNTRY_CHARS: usize = 8;
/// Longueur maximale du lien source (caractères).
pub const MAX_LINK_CHARS: usize = 300;
/// Nombre maximal de bulletins conservés (borne mémoire et affichage).
pub const MAX_BULLETINS: usize = 50;

/// Catégorie d'un bulletin officiel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OfficialCategory {
    /// Vigilance / prévision météorologique.
    Weather,
    /// Catastrophe naturelle (crue, séisme, feu de forêt…).
    Disaster,
    /// Consigne ou communiqué gouvernemental.
    Government,
    /// Alerte sanitaire.
    Health,
    /// Autre source officielle.
    Other,
}

impl OfficialCategory {
    /// Nom de la catégorie tel qu'il circule (sérialisation stable).
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::Weather => "WEATHER",
            Self::Disaster => "DISASTER",
            Self::Government => "GOVERNMENT",
            Self::Health => "HEALTH",
            Self::Other => "OTHER",
        }
    }

    /// Analyse un nom de catégorie. Retourne `None` si inconnu.
    pub fn from_wire_name(name: &str) -> Option<Self> {
        match name {
            "WEATHER" => Some(Self::Weather),
            "DISASTER" => Some(Self::Disaster),
            "GOVERNMENT" => Some(Self::Government),
            "HEALTH" => Some(Self::Health),
            "OTHER" => Some(Self::Other),
            _ => None,
        }
    }

    /// Libellé humain affiché sur le portail.
    pub fn label(self) -> &'static str {
        match self {
            Self::Weather => "🌦️ Météo",
            Self::Disaster => "🌋 Catastrophe naturelle",
            Self::Government => "🏛️ Consigne officielle",
            Self::Health => "🏥 Sanitaire",
            Self::Other => "📋 Information officielle",
        }
    }
}

/// Un bulletin officiel mis en cache (consultable hors-ligne).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfficialBulletin {
    /// Nom de la source (ex. « Météo-France », « VigiCrues », « MétéoSuisse »).
    pub source: String,
    /// Catégorie du contenu.
    pub category: OfficialCategory,
    /// Code pays auquel le bulletin se rapporte (interop `countryCode`).
    /// Vide = portée globale (affiché quel que soit le pays du nœud).
    pub country: String,
    /// Titre court.
    pub title: String,
    /// Corps du message.
    pub body: String,
    /// Horodatage Unix de publication d'origine (0 si inconnu).
    pub published: i64,
    /// Horodatage Unix de la mise en cache locale (ingestion).
    pub fetched: i64,
    /// Lien source (affiché à titre indicatif ; non suivi hors-ligne).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

impl OfficialBulletin {
    /// Construit un bulletin en bornant chaque champ à sa limite. Un lien vide
    /// est normalisé en `None`.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        source: &str,
        category: OfficialCategory,
        country: &str,
        title: &str,
        body: &str,
        published: i64,
        fetched: i64,
        link: Option<&str>,
    ) -> Self {
        let link = link
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| truncate_chars(l, MAX_LINK_CHARS).to_owned());
        Self {
            source: truncate_chars(source.trim(), MAX_SOURCE_CHARS).to_owned(),
            category,
            country: truncate_chars(country.trim(), MAX_COUNTRY_CHARS).to_owned(),
            title: truncate_chars(title.trim(), MAX_TITLE_CHARS).to_owned(),
            body: truncate_chars(body.trim(), MAX_BODY_CHARS).to_owned(),
            published,
            fetched,
            link,
        }
    }

    /// `true` si le bulletin concerne le pays `code` (insensible à la casse) ou
    /// s'il est de portée globale (pays vide).
    #[must_use]
    pub fn concerns_country(&self, code: &str) -> bool {
        self.country.is_empty() || self.country.eq_ignore_ascii_case(code)
    }
}

/// Cache local des bulletins officiels (une seule instance par nœud).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OfficialCache {
    /// Bulletins conservés, triés du plus récent au plus ancien.
    #[serde(default)]
    pub bulletins: Vec<OfficialBulletin>,
    /// Horodatage Unix de la dernière ingestion.
    #[serde(default)]
    pub updated: i64,
}

impl OfficialCache {
    /// Ingère un bulletin : remplace celui de **même source et même titre** s'il
    /// existe (mise à jour), sinon l'ajoute. Retrie par date décroissante et
    /// borne le cache à [`MAX_BULLETINS`] (les plus anciens sont écartés).
    pub fn ingest(&mut self, bulletin: OfficialBulletin) {
        match self
            .bulletins
            .iter_mut()
            .find(|b| b.source == bulletin.source && b.title == bulletin.title)
        {
            Some(slot) => *slot = bulletin,
            None => self.bulletins.push(bulletin),
        }
        // Plus récent d'abord : par publication, puis par date d'ingestion.
        self.bulletins.sort_by(|a, b| {
            b.published
                .cmp(&a.published)
                .then(b.fetched.cmp(&a.fetched))
        });
        self.bulletins.truncate(MAX_BULLETINS);
        self.updated = self.bulletins.iter().map(|b| b.fetched).max().unwrap_or(0);
    }

    /// Bulletins concernant le pays `code` (et ceux de portée globale), dans
    /// l'ordre du cache. Un `code` vide retourne tous les bulletins.
    #[must_use]
    pub fn for_country(&self, code: &str) -> Vec<&OfficialBulletin> {
        self.bulletins
            .iter()
            .filter(|b| code.is_empty() || b.concerns_country(code))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bulletin(source: &str, title: &str, country: &str, published: i64) -> OfficialBulletin {
        OfficialBulletin::new(
            source,
            OfficialCategory::Weather,
            country,
            title,
            "corps",
            published,
            published + 1,
            None,
        )
    }

    #[test]
    fn category_wire_names_round_trip() {
        for c in [
            OfficialCategory::Weather,
            OfficialCategory::Disaster,
            OfficialCategory::Government,
            OfficialCategory::Health,
            OfficialCategory::Other,
        ] {
            assert_eq!(OfficialCategory::from_wire_name(c.wire_name()), Some(c));
        }
        assert_eq!(OfficialCategory::from_wire_name("OVNI"), None);
    }

    #[test]
    fn new_truncates_and_normalizes_link() {
        let long: String = "é".repeat(MAX_TITLE_CHARS + 50);
        let b = OfficialBulletin::new(
            "  Source  ",
            OfficialCategory::Government,
            " fr ",
            &long,
            "x",
            0,
            0,
            Some("   "),
        );
        assert_eq!(b.source, "Source");
        assert_eq!(b.country, "fr");
        assert_eq!(b.title.chars().count(), MAX_TITLE_CHARS);
        // Un lien vide (après trim) devient None.
        assert_eq!(b.link, None);
    }

    #[test]
    fn ingest_dedups_by_source_and_title() {
        let mut cache = OfficialCache::default();
        cache.ingest(bulletin("Météo-France", "Vigilance orange", "fr", 10));
        cache.ingest(bulletin("Météo-France", "Vigilance orange", "fr", 20));
        // Même (source, titre) : mise à jour, pas de doublon.
        assert_eq!(cache.bulletins.len(), 1);
        assert_eq!(cache.bulletins.first().map(|b| b.published), Some(20));
    }

    #[test]
    fn ingest_sorts_recent_first_and_caps() {
        let mut cache = OfficialCache::default();
        for i in 0..(MAX_BULLETINS as i64 + 10) {
            cache.ingest(bulletin("S", &format!("t{i}"), "fr", i));
        }
        assert_eq!(cache.bulletins.len(), MAX_BULLETINS);
        // Le plus récent en tête, le plus ancien conservé est écarté.
        let newest = cache.bulletins.first().map(|b| b.published);
        let second = cache.bulletins.get(1).map(|b| b.published);
        assert!(newest > second);
    }

    #[test]
    fn for_country_includes_global_and_matches_case_insensitive() {
        let mut cache = OfficialCache::default();
        cache.ingest(bulletin("S", "national", "FR", 3));
        cache.ingest(bulletin("S", "global", "", 2));
        cache.ingest(bulletin("S", "suisse", "CH", 1));
        let fr = cache.for_country("fr");
        let titles: Vec<&str> = fr.iter().map(|b| b.title.as_str()).collect();
        assert!(titles.contains(&"national"));
        assert!(titles.contains(&"global"));
        assert!(!titles.contains(&"suisse"));
    }

    #[test]
    fn serde_round_trip() -> Result<(), serde_json::Error> {
        let mut cache = OfficialCache::default();
        cache.ingest(bulletin("VigiCrues", "Crue Seine", "fr", 1_750_000_000));
        let json = serde_json::to_string(&cache)?;
        assert!(json.contains("\"WEATHER\""));
        assert_eq!(
            serde_json::from_str::<OfficialCache>(&json)?
                .bulletins
                .len(),
            1
        );
        Ok(())
    }
}
