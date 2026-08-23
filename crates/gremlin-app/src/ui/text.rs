//! Utilitaires de troncature de texte sûrs en UTF-8.
//!
//! Le rendu de l'interface manipule des chaînes venant de l'utilisateur
//! (noms de dépôts, messages de commit) et des libellés français accentués.
//! Toute troncature par indice d'octet — `&s[0..21]` — panique dès que la
//! coupure tombe au milieu d'un caractère multi-octets, ce qui, avec
//! `panic = "abort"` en release, tue l'application entière depuis la boucle de
//! rendu. Ces helpers raisonnent exclusivement en caractères.

/// Suffixe ajouté lorsqu'une chaîne a été raccourcie.
const ELLIPSIS: &str = "...";

/// Tronque `text` à `max_chars` caractères, sans jamais couper un caractère.
#[must_use]
pub fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    text.chars().take(max_chars).collect()
}

/// Tronque `text` à `max_chars` caractères en signalant la coupure par « ... ».
///
/// Le résultat ne dépasse jamais `max_chars` caractères, points de suspension
/// inclus.
#[must_use]
pub fn truncate_with_ellipsis(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_owned();
    }

    let ellipsis_len = ELLIPSIS.chars().count();
    if max_chars <= ellipsis_len {
        return truncate_chars(ELLIPSIS, max_chars);
    }

    let mut out: String = text.chars().take(max_chars - ellipsis_len).collect();
    out.push_str(ELLIPSIS);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_text_is_untouched() {
        assert_eq!(truncate_with_ellipsis("court", 24), "court");
        assert_eq!(truncate_chars("court", 24), "court");
    }

    #[test]
    fn test_truncation_never_splits_a_multibyte_character() {
        // Reproduit le panic historique : « Réanimer Gremlin (Renaissance) »
        // fait 30 caractères pour 31 octets, et l'ancienne coupure &s[0..21]
        // tombait au milieu d'un caractère selon le contenu.
        let title = "Réanimer Gremlin (Renaissance)";
        let out = truncate_with_ellipsis(title, 24);
        assert_eq!(out.chars().count(), 24);
        assert!(out.ends_with("..."));
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn test_truncation_on_dense_multibyte_input() {
        for text in [
            "ééééééééééééééééééééééééééé",
            "漢字漢字漢字漢字",
            "🐉🐉🐉🐉🐉🐉",
        ] {
            let out = truncate_with_ellipsis(text, 5);
            assert_eq!(out.chars().count(), 5);
        }
    }

    #[test]
    fn test_degenerate_limits_do_not_panic() {
        assert_eq!(truncate_with_ellipsis("Dépôt", 0), "");
        assert_eq!(truncate_with_ellipsis("Dépôt", 1), ".");
        assert_eq!(truncate_with_ellipsis("Dépôt", 3), "...");
        assert_eq!(truncate_chars("Dépôt", 0), "");
    }

    #[test]
    fn test_result_length_is_bounded_for_arbitrary_input() {
        let commit = "fix: gère les caractères « spéciaux » — et l'unicode 🐉";
        for limit in 0..40 {
            assert!(truncate_with_ellipsis(commit, limit).chars().count() <= limit.max(0));
        }
    }
}
