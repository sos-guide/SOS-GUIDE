//! Validation des entrées textuelles destinées à être affichées (anti-XSS).
//!
//! Le portail rend les valeurs de configuration telles quelles côté client (le
//! JS hérité de la v2.5 injecte dans le DOM). Refuser à l'écriture les
//! caractères dangereux est la défense la plus fiable : aucune valeur stockée
//! ne peut alors injecter du HTML, quelle que soit la façon dont une page la
//! réutilise plus tard.

/// Pourquoi une chaîne est refusée.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextError {
    /// Dépasse la longueur maximale autorisée (en caractères Unicode).
    TooLong,
    /// Contient un caractère de contrôle interdit (hors `\t`, `\n`, `\r`).
    ControlChar,
    /// Contient `<` ou `>` — vecteur d'injection HTML/XSS.
    HtmlUnsafe,
}

impl TextError {
    /// Libellé court (français) pour une réponse d'API.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            TextError::TooLong => "champ trop long",
            TextError::ControlChar => "caractère de contrôle interdit",
            TextError::HtmlUnsafe => "caractère HTML interdit (< ou >)",
        }
    }
}

/// Valide une chaîne libre **affichée** dans le portail : longueur bornée,
/// aucun caractère de contrôle (hors tabulation/retours), aucun `<`/`>`.
///
/// # Errors
/// Renvoie [`TextError`] décrivant la première règle enfreinte.
pub fn validate_text(s: &str, max_len: usize) -> Result<(), TextError> {
    let mut count = 0usize;
    for c in s.chars() {
        count += 1;
        if count > max_len {
            return Err(TextError::TooLong);
        }
        if c == '<' || c == '>' {
            return Err(TextError::HtmlUnsafe);
        }
        // WHY: `is_control` couvre aussi NUL et les caractères d'échappement ;
        // on tolère uniquement la mise en forme blanche usuelle.
        if c.is_control() && !matches!(c, '\t' | '\n' | '\r') {
            return Err(TextError::ControlChar);
        }
    }
    Ok(())
}

/// Valide une chaîne **secrète** jamais rendue dans une page (mot de passe
/// WiFi…) : longueur et caractères de contrôle bornés, mais `<`/`>` autorisés
/// (une clé WPA2 peut légitimement les contenir).
///
/// # Errors
/// Renvoie [`TextError::TooLong`] ou [`TextError::ControlChar`].
pub fn validate_secret(s: &str, max_len: usize) -> Result<(), TextError> {
    let mut count = 0usize;
    for c in s.chars() {
        count += 1;
        if count > max_len {
            return Err(TextError::TooLong);
        }
        if c.is_control() && !matches!(c, '\t' | '\n' | '\r') {
            return Err(TextError::ControlChar);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_accepted() {
        assert_eq!(validate_text("Mairie de Sévrier", 200), Ok(()));
        assert_eq!(validate_text("ligne 1\nligne 2\tfin", 200), Ok(()));
        assert_eq!(validate_text("", 200), Ok(()));
    }

    #[test]
    fn html_is_rejected() {
        assert_eq!(
            validate_text("<script>alert(1)</script>", 200),
            Err(TextError::HtmlUnsafe)
        );
        assert_eq!(validate_text("a > b", 200), Err(TextError::HtmlUnsafe));
    }

    #[test]
    fn control_chars_rejected() {
        assert_eq!(validate_text("a\0b", 200), Err(TextError::ControlChar));
        assert_eq!(
            validate_text("esc\x1b[2J", 200),
            Err(TextError::ControlChar)
        );
    }

    #[test]
    fn length_is_bounded_by_chars_not_bytes() {
        // 3 caractères accentués (≥ 2 octets chacun) tiennent dans max=3.
        assert_eq!(validate_text("éàü", 3), Ok(()));
        assert_eq!(validate_text("éàü", 2), Err(TextError::TooLong));
    }

    #[test]
    fn secret_allows_angle_brackets_but_not_control() {
        assert_eq!(validate_secret("p4ss<word>!", 63), Ok(()));
        assert_eq!(validate_secret("a\0b", 63), Err(TextError::ControlChar));
        assert_eq!(validate_secret("trop long", 3), Err(TextError::TooLong));
    }
}
