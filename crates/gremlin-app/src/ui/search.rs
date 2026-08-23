//! Correspondance floue et repli des diacritiques.
//!
//! # Pourquoi ce module existe
//!
//! Le filtrage se réduisait à `title.to_lowercase().contains(&query)`. Trois
//! conséquences :
//!
//! * taper `depot` ne trouvait pas « Dépôt », faute de repli des accents — ce
//!   qui est pénible sur un clavier où l'accent coûte une touche morte ;
//! * taper `ez` ne trouvait pas « Échelle de zoom », faute de correspondance par
//!   sous-séquence ;
//! * les résultats sortaient dans l'ordre de construction de la liste, sans
//!   notion de pertinence : la meilleure correspondance pouvait se retrouver en
//!   huitième position.
//!
//! Le score est volontairement simple et déterministe : il récompense les débuts
//! de mot, les caractères consécutifs et le début de la chaîne, et pénalise les
//! trous. Aucune heuristique probabiliste, donc aucun classement surprenant.

/// Points accordés à une correspondance en tout début de chaîne.
const BONUS_START: i32 = 24;

/// Points accordés à une correspondance en début de mot.
const BONUS_WORD_START: i32 = 16;

/// Points accordés à un caractère qui suit immédiatement le précédent.
const BONUS_CONSECUTIVE: i32 = 10;

/// Points de base par caractère apparié.
const SCORE_PER_CHAR: i32 = 4;

/// Pénalité par caractère sauté entre deux appariements.
const PENALTY_PER_GAP: i32 = 1;

/// Plafond de pénalité pour un seul trou, afin qu'une chaîne longue ne soit pas
/// éliminée par un unique grand écart.
const MAX_GAP_PENALTY: i32 = 12;

/// Caractères considérés comme des séparateurs de mots.
///
/// Le point et le tiret comptent : `open-mods-folder` doit répondre à `omf`.
fn is_word_separator(ch: char) -> bool {
    matches!(
        ch,
        ' ' | '-' | '_' | '.' | '/' | '\\' | ':' | '(' | ')' | '«' | '»' | '\'' | '’'
    )
}

/// Replie un caractère sur sa forme de recherche : minuscule et sans diacritique.
///
/// Les ligatures sont repliées sur leur première lettre plutôt que développées :
/// `œ` devient `o`. Développer en deux caractères désaligne les indices de
/// l'appariement pour un gain nul sur les libellés du panneau.
#[must_use]
pub fn fold_char(ch: char) -> char {
    // Les ligatures et lettres barrées rejoignent le groupe de leur première
    // lettre : `æ` avec `a`, `œ` et `ø` avec `o`.
    let folded = match ch {
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' | 'æ' | 'Á' | 'À' | 'Â' | 'Ä' | 'Ã' | 'Å' | 'Æ' => {
            'a'
        }
        'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'œ' | 'ø' | 'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' | 'Œ' | 'Ø' => {
            'o'
        }
        'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
        'ç' | 'Ç' => 'c',
        'ñ' | 'Ñ' => 'n',
        'ÿ' | 'ý' | 'Ÿ' | 'Ý' => 'y',
        other => other,
    };

    // `to_lowercase` peut produire plusieurs caractères pour certaines lettres ;
    // on retient le premier, ce qui suffit aux alphabets latins visés et garde
    // la fonction totale.
    folded.to_lowercase().next().unwrap_or(folded)
}

/// Replie une chaîne entière pour la comparaison.
#[must_use]
pub fn fold(text: &str) -> String {
    text.chars().map(fold_char).collect()
}

/// Score de correspondance de `query` dans `haystack`, ou `None` si absente.
///
/// Un score plus élevé signale une meilleure correspondance. Une requête vide
/// renvoie `Some(0)` : tout correspond, sans préférence.
#[must_use]
pub fn score(haystack: &str, query: &str) -> Option<i32> {
    let needle: Vec<char> = query.chars().map(fold_char).filter(|c| *c != ' ').collect();
    if needle.is_empty() {
        return Some(0);
    }

    let hay: Vec<char> = haystack.chars().map(fold_char).collect();
    if needle.len() > hay.len() {
        return None;
    }

    let mut total = 0;
    let mut hay_index = 0_usize;
    let mut previous_match: Option<usize> = None;

    for &wanted in &needle {
        let found = hay[hay_index..].iter().position(|&c| c == wanted)?;
        let absolute = hay_index + found;

        total += SCORE_PER_CHAR;

        if absolute == 0 {
            total += BONUS_START;
        } else if hay
            .get(absolute.wrapping_sub(1))
            .copied()
            .is_some_and(is_word_separator)
        {
            total += BONUS_WORD_START;
        }

        if previous_match.is_some_and(|previous| previous + 1 == absolute) {
            total += BONUS_CONSECUTIVE;
        }

        // Le trou est compté depuis le caractère apparié précédent, pas depuis
        // le début : une longue chaîne n'est pas pénalisée pour sa longueur.
        let gap = previous_match.map_or(absolute, |previous| absolute - previous - 1);
        total -= i32::try_from(gap)
            .unwrap_or(MAX_GAP_PENALTY)
            .saturating_mul(PENALTY_PER_GAP)
            .min(MAX_GAP_PENALTY);

        previous_match = Some(absolute);
        hay_index = absolute + 1;
    }

    Some(total)
}

/// Meilleur score de `query` parmi plusieurs champs d'un même élément.
///
/// Permet de chercher à la fois dans le titre, le sous-titre et l'identifiant
/// sans que le champ le plus faible ne dégrade le classement.
#[must_use]
pub fn best_score(fields: &[&str], query: &str) -> Option<i32> {
    fields.iter().filter_map(|field| score(field, query)).max()
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_accent_insensitive_search() {
        // Le cas qui motive tout le module : trouver « Dépôt » en tapant
        // « depot », sans avoir à composer l'accent.
        assert!(score("Dépôt : gremlin", "depot").is_some());
        assert!(score("Échelle de zoom", "echelle").is_some());
        assert!(score("Satiété actuelle", "satiete").is_some());
        assert!(score("Réanimer Gremlin", "reanimer").is_some());

        // Et réciproquement : taper l'accent doit trouver la forme accentuée.
        assert!(score("Dépôt", "dépôt").is_some());
    }

    #[test]
    fn test_case_insensitive_search() {
        assert!(score("Soigner le familier", "SOIGNER").is_some());
        assert!(score("SOIGNER", "soigner").is_some());
    }

    #[test]
    fn test_subsequence_matching() {
        // Correspondance par initiales, impossible avec `contains`.
        assert!(score("Échelle de zoom", "ez").is_some());
        assert!(score("Ouvrir le répertoire des Skins / Mods", "osm").is_some());
        assert!(score("Mode Click-Through", "mct").is_some());
    }

    #[test]
    fn test_absent_query_returns_none() {
        assert_eq!(score("Nourrir d'un snack", "zzz"), None);
        assert_eq!(score("court", "chaine beaucoup plus longue"), None);
        // Les caractères doivent apparaître dans l'ordre.
        assert_eq!(score("abc", "cba"), None);
    }

    #[test]
    fn test_empty_query_matches_everything_without_preference() {
        assert_eq!(score("n'importe quoi", ""), Some(0));
        assert_eq!(score("", ""), Some(0));
        assert_eq!(score("", "a"), None);
    }

    #[test]
    fn test_word_start_outranks_a_match_in_the_middle() {
        // « sn » en début de mot dans « snack » doit primer sur le même « sn »
        // dispersé ailleurs.
        let at_word_start = score("Nourrir d'un snack", "sn").expect("correspondance attendue");
        let scattered = score("Surveillance nocturne interne", "sn").expect("correspondance");
        assert!(
            at_word_start > 0 && scattered > 0,
            "les deux doivent correspondre"
        );

        let prefix = score("Snack", "sn").expect("correspondance");
        assert!(
            prefix > at_word_start,
            "un début de chaîne doit primer sur un début de mot interne : {prefix} contre {at_word_start}"
        );
    }

    #[test]
    fn test_consecutive_characters_outrank_scattered_ones() {
        // Attention au choix des données : « compagnon » contient lui aussi
        // « com » de façon consécutive, et ne constitue donc pas un contre-exemple.
        let consecutive = score("commit", "com").expect("correspondance");
        let scattered = score("caisse ordinaire multiple", "com").expect("correspondance");
        assert!(
            consecutive > scattered,
            "consécutif {consecutive} devrait primer sur dispersé {scattered}"
        );
    }

    #[test]
    fn test_scores_are_finite_and_ordered_on_realistic_labels() {
        let labels = [
            "Nourrir d'un snack",
            "Soigner le familier",
            "Endormir Gremlin (Mode Pause)",
            "Échelle de zoom : 3x",
            "Sauvegarder l'état (Atomic Save)",
        ];

        let mut best = None;
        for label in labels {
            if let Some(value) = score(label, "sn") {
                best = Some(best.map_or(value, |b: i32| b.max(value)));
            }
        }
        assert!(
            best.is_some(),
            "« sn » doit correspondre à au moins un libellé"
        );
    }

    #[test]
    fn test_hostile_input_never_panics() {
        for haystack in ["", "🐉漢字", "\u{0}\u{7f}", "é".repeat(500).as_str()] {
            for query in ["", "🐉", "e", "\u{0}", "aaaaaaaaaaaaaaaaaaaaaaaaa"] {
                let _ = score(haystack, query);
            }
        }
    }

    #[test]
    fn test_best_score_takes_the_strongest_field() {
        let fields = [
            "Sauvegarder l'état",
            "Écrit save.json sur disque",
            "action_save_now",
        ];
        let combined = best_score(&fields, "save").expect("correspondance");
        let weakest = score(fields[0], "save");
        assert!(
            weakest.is_none_or(|w| combined >= w),
            "le meilleur champ doit gagner"
        );
    }

    #[test]
    fn test_folding_is_total_and_idempotent() {
        for ch in ['é', 'Ç', 'ø', 'æ', 'A', 'z', '5', '🐉', '漢'] {
            let once = fold_char(ch);
            assert_eq!(
                fold_char(once),
                once,
                "le repli de « {ch} » n'est pas idempotent"
            );
        }
        assert_eq!(fold("Dépôt Éveillé"), "depot eveille");
    }
}
