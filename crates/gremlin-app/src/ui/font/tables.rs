//! Dessins des glyphes, écrits à la main.
//!
//! # Comment lire et modifier ce fichier
//!
//! Chaque entrée associe un caractère, l'ordonnée de sa première ligne dessinée
//! dans la cellule, puis le dessin lui-même : une chaîne par ligne de pixels.
//! Les quatre niveaux de couverture sont `.` (vide), `:` (léger), `+` (marqué)
//! et `#` (plein). Modifier une lettre se fait donc en éditant son dessin, sans
//! outil ni régénération.
//!
//! # Métrique verticale du corps 8×15
//!
//! | Lignes | Rôle |
//! | --- | --- |
//! | 0-1 | accents des capitales |
//! | 2-3 | accents des minuscules |
//! | 2-11 | hauteur de capitale (10 px) |
//! | 5-11 | hauteur d'œil des minuscules (7 px) |
//! | 11 | ligne de base |
//! | 12-14 | jambages descendants et cédille |
//!
//! La ligne 4 reste libre : elle sépare l'accent du corps de la lettre, sans
//! quoi « é » devient une tache aux petits corps.
//!
//! # État du dessin
//!
//! Deux corps sont dessinés : le corps moyen 8×15 ici même, et le corps compact
//! 6×11 dans [`super::small`]. Le corps 11×20 déclaré par [`FontSize`] reste à
//! tracer ; en attendant, le moteur sert le corps moyen à sa place (voir `face`
//! dans le module parent), ce qui donne aux densités élevées un texte un peu
//! plus petit que la maquette ne le prévoit, sans rien casser. Le sélecteur de
//! corps de `layout.rs` est indépendant de cet état et n'aura pas à changer.
//!
//! # Ajouter un glyphe
//!
//! Le dessiner **dans les deux corps** : un caractère présent dans un seul sort
//! en glyphe de repli — un rectangle creux — partout où l'autre corps est servi,
//! et cela ne se voit qu'en regardant la planche. Les deux tables sont bornées
//! par `test_every_drawing_stays_inside_its_cell`, et la planche de police rend
//! désormais les deux corps : y ajouter le nouveau caractère fait partie du
//! travail, sans quoi il reste invisible au contrôle.

use super::{small, Face, Glyph, SPACE_WIDTH};
use crate::ui::layout::FontSize;
use std::collections::HashMap;

/// Un glyphe dessiné : caractère, première ligne occupée, dessin.
pub(super) type Drawing = (char, i32, &'static [&'static str]);

/// Nature d'une marque diacritique, qui détermine où elle se pose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkKind {
    /// Au-dessus de la lettre : la hauteur dépend de la casse.
    Above,
    /// Sous la ligne de base.
    Below,
}

/// Une marque diacritique dessinée.
type MarkDrawing = (&'static str, MarkKind, &'static [&'static str]);

/// Hauteurs d'accueil des marques diacritiques, propres à un corps.
///
/// Elles ne peuvent pas être partagées entre corps : dans une cellule de onze
/// lignes, un accent posé à la ligne 2 tomberait sur la lettre, alors que c'est
/// exactement sa place dans une cellule de quinze.
struct FaceGeometry {
    /// Ligne d'accueil d'une marque posée sur une capitale.
    uppercase: i32,
    /// Ligne d'accueil d'une marque posée sur une minuscule.
    lowercase: i32,
    /// Ligne d'accueil d'une marque posée sous la ligne de base.
    below: i32,
}

/// Métrique d'accents du corps moyen 8×15 : ligne de base à la ligne 11.
const MEDIUM_GEOMETRY: FaceGeometry = FaceGeometry {
    uppercase: 0,
    lowercase: 2,
    below: 12,
};

/// Métrique d'accents du corps compact 6×11 : ligne de base à la ligne 8.
const SMALL_GEOMETRY: FaceGeometry = FaceGeometry {
    uppercase: 0,
    lowercase: 1,
    below: 9,
};

// ==========================================================================
// Marques diacritiques
// ==========================================================================

#[rustfmt::skip]
const MARKS: &[MarkDrawing] = &[
    ("aigu", MarkKind::Above, &[
        "..+#",
        ".#+.",
    ]),
    ("grave", MarkKind::Above, &[
        "#+..",
        ".+#.",
    ]),
    ("circonflexe", MarkKind::Above, &[
        ".##.",
        "#..#",
    ]),
    ("trema", MarkKind::Above, &[
        "#..#",
        "#..#",
    ]),
    ("cedille", MarkKind::Below, &[
        "..#.",
        ".##+",
    ]),
];

/// Compositions : caractère accentué, lettre de base, nom de la marque.
///
/// Composer plutôt que dessiner évite vingt-six tracés supplémentaires, garantit
/// des accents identiques d'une lettre à l'autre, et lève surtout la limite
/// historique sur les capitales accentuées.
const COMPOSITIONS: &[(char, char, &str)] = &[
    ('é', 'e', "aigu"),
    ('è', 'e', "grave"),
    ('ê', 'e', "circonflexe"),
    ('ë', 'e', "trema"),
    ('á', 'a', "aigu"),
    ('à', 'a', "grave"),
    ('â', 'a', "circonflexe"),
    ('ä', 'a', "trema"),
    ('í', 'i', "aigu"),
    ('ì', 'i', "grave"),
    ('î', 'i', "circonflexe"),
    ('ï', 'i', "trema"),
    ('ó', 'o', "aigu"),
    ('ò', 'o', "grave"),
    ('ô', 'o', "circonflexe"),
    ('ö', 'o', "trema"),
    ('ú', 'u', "aigu"),
    ('ù', 'u', "grave"),
    ('û', 'u', "circonflexe"),
    ('ü', 'u', "trema"),
    ('ç', 'c', "cedille"),
    ('É', 'E', "aigu"),
    ('È', 'E', "grave"),
    ('Ê', 'E', "circonflexe"),
    ('Ë', 'E', "trema"),
    ('À', 'A', "grave"),
    ('Â', 'A', "circonflexe"),
    ('Ä', 'A', "trema"),
    ('Î', 'I', "circonflexe"),
    ('Ï', 'I', "trema"),
    ('Ô', 'O', "circonflexe"),
    ('Ö', 'O', "trema"),
    ('Ù', 'U', "grave"),
    ('Û', 'U', "circonflexe"),
    ('Ü', 'U', "trema"),
    ('Ç', 'C', "cedille"),
];

/// Replis vers un caractère dessiné, pour les signes proches.
const FOLDS: &[(char, char)] = &[
    ('œ', 'o'),
    ('Œ', 'O'),
    ('æ', 'a'),
    ('Æ', 'A'),
    ('ø', 'o'),
    ('Ø', 'O'),
    ('ñ', 'n'),
    ('Ñ', 'N'),
    ('ÿ', 'y'),
    ('“', '"'),
    ('”', '"'),
    ('‘', '\''),
    ('–', '-'),
    ('\u{2011}', '-'),
    ('\u{202f}', ' '),
    ('\u{2009}', ' '),
];

// ==========================================================================
// Corps moyen 8×15 — capitales
// ==========================================================================

#[rustfmt::skip]
const MEDIUM_UPPERCASE: &[Drawing] = &[
    ('A', 2, &[
        "..#..",
        ".#.#.",
        ".#.#.",
        ".#.#.",
        "#...#",
        "#####",
        "#...#",
        "#...#",
        "#...#",
        "#...#",
    ]),
    ('B', 2, &[
        "#####.",
        "#....#",
        "#....#",
        "#....#",
        "#####.",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#####.",
    ]),
    ('C', 2, &[
        ".####.",
        "#+..+#",
        "#.....",
        "#.....",
        "#.....",
        "#.....",
        "#.....",
        "#.....",
        "#+..+#",
        ".####.",
    ]),
    ('D', 2, &[
        "#####.",
        "#....+",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#....+",
        "#####.",
    ]),
    ('E', 2, &[
        "######",
        "#.....",
        "#.....",
        "#.....",
        "#####.",
        "#.....",
        "#.....",
        "#.....",
        "#.....",
        "######",
    ]),
    ('F', 2, &[
        "######",
        "#.....",
        "#.....",
        "#.....",
        "#####.",
        "#.....",
        "#.....",
        "#.....",
        "#.....",
        "#.....",
    ]),
    ('G', 2, &[
        ".####.",
        "#+..+#",
        "#.....",
        "#.....",
        "#..###",
        "#....#",
        "#....#",
        "#....#",
        "#+..+#",
        ".####.",
    ]),
    ('H', 2, &[
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "######",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
    ]),
    ('I', 2, &[
        ".#.",
        ".#.",
        ".#.",
        ".#.",
        ".#.",
        ".#.",
        ".#.",
        ".#.",
        ".#.",
        ".#.",
    ]),
    ('J', 2, &[
        "....#",
        "....#",
        "....#",
        "....#",
        "....#",
        "....#",
        "....#",
        "#...#",
        "#+.+#",
        ".###.",
    ]),
    ('K', 2, &[
        "#....#",
        "#...#.",
        "#..#..",
        "#.#...",
        "###...",
        "#..#..",
        "#..+#.",
        "#...#.",
        "#....#",
        "#....#",
    ]),
    ('L', 2, &[
        "#.....",
        "#.....",
        "#.....",
        "#.....",
        "#.....",
        "#.....",
        "#.....",
        "#.....",
        "#.....",
        "######",
    ]),
    ('M', 2, &[
        "#.....#",
        "##...##",
        "#+#.#+#",
        "#.#.#.#",
        "#.+#+.#",
        "#..#..#",
        "#.....#",
        "#.....#",
        "#.....#",
        "#.....#",
    ]),
    ('N', 2, &[
        "#....#",
        "##...#",
        "#+#..#",
        "#.#..#",
        "#.+#.#",
        "#..#.#",
        "#..+##",
        "#...##",
        "#....#",
        "#....#",
    ]),
    ('O', 2, &[
        ".####.",
        "#+..+#",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#+..+#",
        ".####.",
    ]),
    ('P', 2, &[
        "#####.",
        "#....#",
        "#....#",
        "#....#",
        "#####.",
        "#.....",
        "#.....",
        "#.....",
        "#.....",
        "#.....",
    ]),
    ('Q', 2, &[
        ".####.",
        "#+..+#",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#..#.#",
        "#+..+#",
        ".####+",
        "....+#",
    ]),
    ('R', 2, &[
        "#####.",
        "#....#",
        "#....#",
        "#....#",
        "#####.",
        "#..#..",
        "#..+#.",
        "#...#.",
        "#....#",
        "#....#",
    ]),
    ('S', 2, &[
        ".####.",
        "#+..+#",
        "#.....",
        "#+....",
        ".####.",
        "....+#",
        ".....#",
        "#....#",
        "#+..+#",
        ".####.",
    ]),
    ('T', 2, &[
        "#####",
        "..#..",
        "..#..",
        "..#..",
        "..#..",
        "..#..",
        "..#..",
        "..#..",
        "..#..",
        "..#..",
    ]),
    ('U', 2, &[
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#+..+#",
        ".####.",
    ]),
    ('V', 2, &[
        "#...#",
        "#...#",
        "#...#",
        "#...#",
        "#...#",
        "+#.#+",
        ".#.#.",
        ".#.#.",
        ".+#+.",
        "..#..",
    ]),
    ('W', 2, &[
        "#.....#",
        "#.....#",
        "#.....#",
        "#..#..#",
        "#..#..#",
        "#..#..#",
        "#.+#+.#",
        "##...##",
        "#+...+#",
        ".#...#.",
    ]),
    ('X', 2, &[
        "#...#",
        "+#.#+",
        ".#.#.",
        ".+#+.",
        "..#..",
        "..#..",
        ".+#+.",
        ".#.#.",
        "+#.#+",
        "#...#",
    ]),
    ('Y', 2, &[
        "#...#",
        "+#.#+",
        ".#.#.",
        ".+#+.",
        "..#..",
        "..#..",
        "..#..",
        "..#..",
        "..#..",
        "..#..",
    ]),
    ('Z', 2, &[
        "######",
        "....+#",
        "....#.",
        "...#..",
        "..##..",
        "..#...",
        ".#....",
        "#+....",
        "#.....",
        "######",
    ]),
];

// ==========================================================================
// Corps moyen 8×15 — minuscules
// ==========================================================================

#[rustfmt::skip]
const MEDIUM_LOWERCASE: &[Drawing] = &[
    ('a', 5, &[
        ".####.",
        "+...+#",
        "....+#",
        ".#####",
        "#....#",
        "#...+#",
        ".####+",
    ]),
    ('b', 2, &[
        "#.....",
        "#.....",
        "#.....",
        "#.....",
        "#####.",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#####.",
    ]),
    ('c', 5, &[
        ".####.",
        "#+..+#",
        "#.....",
        "#.....",
        "#.....",
        "#+..+#",
        ".####.",
    ]),
    ('d', 2, &[
        ".....#",
        ".....#",
        ".....#",
        ".....#",
        ".#####",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        ".#####",
    ]),
    ('e', 5, &[
        ".####.",
        "#+..+#",
        "#....#",
        "######",
        "#.....",
        "#+..+#",
        ".####.",
    ]),
    ('f', 2, &[
        "..###",
        ".#+..",
        ".#...",
        "####.",
        ".#...",
        ".#...",
        ".#...",
        ".#...",
        ".#...",
        ".#...",
    ]),
    ('g', 5, &[
        ".#####",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        ".#####",
        ".....#",
        ".....#",
        "#+..+#",
        ".####.",
    ]),
    ('h', 2, &[
        "#.....",
        "#.....",
        "#.....",
        "#.....",
        "#####.",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
    ]),
    ('i', 2, &[
        ".#.",
        ".#.",
        "...",
        ".#.",
        ".#.",
        ".#.",
        ".#.",
        ".#.",
        ".#.",
        ".#.",
    ]),
    ('j', 2, &[
        "..#.",
        "..#.",
        "....",
        "..#.",
        "..#.",
        "..#.",
        "..#.",
        "..#.",
        "..#.",
        "..#.",
        "..#.",
        "#++#",
        ".##.",
    ]),
    ('k', 2, &[
        "#.....",
        "#.....",
        "#.....",
        "#.....",
        "#...#.",
        "#..#..",
        "###...",
        "#..#..",
        "#...#.",
        "#....#",
    ]),
    ('l', 2, &[
        ".#.",
        ".#.",
        ".#.",
        ".#.",
        ".#.",
        ".#.",
        ".#.",
        ".#.",
        ".#.",
        ".#.",
    ]),
    ('m', 5, &[
        "####+####",
        "#...#...#",
        "#...#...#",
        "#...#...#",
        "#...#...#",
        "#...#...#",
        "#...#...#",
    ]),
    ('n', 5, &[
        "#####.",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
    ]),
    ('o', 5, &[
        ".####.",
        "#+..+#",
        "#....#",
        "#....#",
        "#....#",
        "#+..+#",
        ".####.",
    ]),
    ('p', 5, &[
        "#####.",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#####.",
        "#.....",
        "#.....",
        "#.....",
        "#.....",
    ]),
    ('q', 5, &[
        ".#####",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        ".#####",
        ".....#",
        ".....#",
        ".....#",
        ".....#",
    ]),
    ('r', 5, &[
        "#.###",
        "##+..",
        "#....",
        "#....",
        "#....",
        "#....",
        "#....",
    ]),
    ('s', 5, &[
        ".#####",
        "#+....",
        "#.....",
        ".####.",
        "....+#",
        "....+#",
        "#####.",
    ]),
    ('t', 2, &[
        ".#...",
        ".#...",
        ".#...",
        "####.",
        ".#...",
        ".#...",
        ".#...",
        ".#...",
        ".#..+",
        ".+###",
    ]),
    ('u', 5, &[
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#....#",
        "#...+#",
        ".#####",
    ]),
    ('v', 5, &[
        "#....#",
        "+#..#+",
        ".#..#.",
        ".+##+.",
        "..##..",
        "..##..",
        "..##..",
    ]),
    ('w', 5, &[
        "#.....#",
        "#..#..#",
        "#..#..#",
        "#.+#+.#",
        "#.#.#.#",
        "##...##",
        "#+...+#",
    ]),
    ('x', 5, &[
        "#....#",
        "+#..#+",
        ".+##+.",
        "..##..",
        ".+##+.",
        "+#..#+",
        "#....#",
    ]),
    ('y', 5, &[
        "#....#",
        "#....#",
        "#....#",
        "+#..#+",
        ".+##+.",
        "..##..",
        "..##..",
        "..#...",
        ".##...",
        "##....",
    ]),
    ('z', 5, &[
        "######",
        "....#.",
        "...#..",
        "..##..",
        ".#....",
        "#.....",
        "######",
    ]),
];

// ==========================================================================
// Corps moyen 8×15 — chiffres
// ==========================================================================

#[rustfmt::skip]
const MEDIUM_DIGITS: &[Drawing] = &[
    // Zéro sans barre oblique : la diagonale, lisible sur un terminal en
    // gros corps, se confondait avec un « théta » à la taille du texte
    // courant. La distinction avec « O » repose sur la largeur, le zéro
    // occupant cinq colonnes contre six pour la capitale.
    ('0', 2, &[
        ".###.",
        "#...#",
        "#...#",
        "#...#",
        "#...#",
        "#...#",
        "#...#",
        "#...#",
        "#...#",
        ".###.",
    ]),
    ('1', 2, &[
        "..#..",
        ".##..",
        "#.#..",
        "..#..",
        "..#..",
        "..#..",
        "..#..",
        "..#..",
        "..#..",
        "#####",
    ]),
    ('2', 2, &[
        ".###.",
        "#...#",
        "....#",
        "....#",
        "...#.",
        "..#..",
        ".#...",
        "#....",
        "#....",
        "#####",
    ]),
    ('3', 2, &[
        ".###.",
        "#...#",
        "....#",
        "....#",
        "..##.",
        "....#",
        "....#",
        "....#",
        "#...#",
        ".###.",
    ]),
    ('4', 2, &[
        "...#.",
        "..##.",
        ".#.#.",
        ".#.#.",
        "#..#.",
        "#..#.",
        "#####",
        "...#.",
        "...#.",
        "...#.",
    ]),
    ('5', 2, &[
        "#####",
        "#....",
        "#....",
        "#....",
        "####.",
        "....#",
        "....#",
        "....#",
        "#...#",
        ".###.",
    ]),
    ('6', 2, &[
        "..##.",
        ".#...",
        "#....",
        "#....",
        "####.",
        "#...#",
        "#...#",
        "#...#",
        "#...#",
        ".###.",
    ]),
    ('7', 2, &[
        "#####",
        "....#",
        "....#",
        "...#.",
        "...#.",
        "..#..",
        "..#..",
        ".#...",
        ".#...",
        "#....",
    ]),
    ('8', 2, &[
        ".###.",
        "#...#",
        "#...#",
        "#...#",
        ".###.",
        "#...#",
        "#...#",
        "#...#",
        "#...#",
        ".###.",
    ]),
    ('9', 2, &[
        ".###.",
        "#...#",
        "#...#",
        "#...#",
        "#...#",
        ".####",
        "....#",
        "....#",
        "...#.",
        ".##..",
    ]),
];

// ==========================================================================
// Corps moyen 8×15 — ponctuation et signes d'interface
// ==========================================================================

#[rustfmt::skip]
const MEDIUM_SYMBOLS: &[Drawing] = &[
    ('.', 10, &[
        "##",
        "##",
    ]),
    (',', 10, &[
        "##",
        "##",
        ".#",
        "#+",
    ]),
    (':', 6, &[
        "##",
        "##",
        "..",
        "..",
        "##",
        "##",
    ]),
    (';', 6, &[
        "##",
        "##",
        "..",
        "..",
        "##",
        "##",
        ".#",
        "#+",
    ]),
    ('!', 2, &[
        "##",
        "##",
        "##",
        "##",
        "##",
        "##",
        "##",
        "..",
        "##",
        "##",
    ]),
    ('?', 2, &[
        ".####.",
        "#+..+#",
        ".....#",
        "....+#",
        "...##.",
        "..##..",
        "..##..",
        "......",
        "..##..",
        "..##..",
    ]),
    ('\'', 2, &[
        "##",
        "##",
        "#+",
    ]),
    ('"', 2, &[
        "##.##",
        "##.##",
        "#+.#+",
    ]),
    ('(', 2, &[
        "..##",
        ".##.",
        ".#..",
        "##..",
        "##..",
        "##..",
        "##..",
        ".#..",
        ".##.",
        "..##",
    ]),
    (')', 2, &[
        "##..",
        ".##.",
        "..#.",
        "..##",
        "..##",
        "..##",
        "..##",
        "..#.",
        ".##.",
        "##..",
    ]),
    ('[', 2, &[
        "###",
        "##.",
        "##.",
        "##.",
        "##.",
        "##.",
        "##.",
        "##.",
        "##.",
        "###",
    ]),
    (']', 2, &[
        "###",
        ".##",
        ".##",
        ".##",
        ".##",
        ".##",
        ".##",
        ".##",
        ".##",
        "###",
    ]),
    ('{', 2, &[
        "..##",
        ".##.",
        ".##.",
        ".##.",
        "##..",
        "##..",
        ".##.",
        ".##.",
        ".##.",
        "..##",
    ]),
    ('}', 2, &[
        "##..",
        ".##.",
        ".##.",
        ".##.",
        "..##",
        "..##",
        ".##.",
        ".##.",
        ".##.",
        "##..",
    ]),
    ('-', 8, &[
        "####",
        "####",
    ]),
    ('_', 12, &[
        "######",
        "######",
    ]),
    ('+', 6, &[
        "..##..",
        "..##..",
        "######",
        "######",
        "..##..",
        "..##..",
    ]),
    ('=', 6, &[
        "######",
        "######",
        "......",
        "......",
        "######",
        "######",
    ]),
    ('*', 3, &[
        "#.#.#",
        ".###.",
        "#####",
        ".###.",
        "#.#.#",
    ]),
    ('/', 2, &[
        "....##",
        "....##",
        "...##.",
        "...##.",
        "..##..",
        "..##..",
        ".##...",
        ".##...",
        "##....",
        "##....",
    ]),
    ('\\', 2, &[
        "##....",
        "##....",
        ".##...",
        ".##...",
        "..##..",
        "..##..",
        "...##.",
        "...##.",
        "....##",
        "....##",
    ]),
    ('|', 2, &[
        "##",
        "##",
        "##",
        "##",
        "##",
        "##",
        "##",
        "##",
        "##",
        "##",
    ]),
    ('<', 4, &[
        "...#",
        "..##",
        ".##.",
        "##..",
        "##..",
        ".##.",
        "..##",
        "...#",
    ]),
    ('>', 4, &[
        "#...",
        "##..",
        ".##.",
        "..##",
        "..##",
        ".##.",
        "##..",
        "#...",
    ]),
    ('%', 2, &[
        "##...#",
        "##..#.",
        "...#..",
        "...#..",
        "..#...",
        "..#...",
        ".#....",
        "#...##",
        "....##",
        "......",
    ]),
    ('&', 2, &[
        ".###..",
        "#...#.",
        "#...#.",
        ".###..",
        "#####.",
        "#...#.",
        "#...#+",
        "#....#",
        "#...+#",
        ".###.#",
    ]),
    ('$', 1, &[
        "..##..",
        ".####.",
        "#+##+#",
        "#.##..",
        ".####.",
        "..##+#",
        "..##.#",
        "#+##+#",
        ".####.",
        "..##..",
        "..##..",
    ]),
    ('#', 3, &[
        ".##.##.",
        ".##.##.",
        "#######",
        ".##.##.",
        ".##.##.",
        "#######",
        ".##.##.",
        ".##.##.",
    ]),
    ('@', 2, &[
        ".####.",
        "#+..+#",
        "#.##.#",
        "#.#..#",
        "#.#..#",
        "#.####",
        "#.....",
        "#+....",
        ".####.",
        "......",
    ]),
    ('^', 2, &[
        "..##..",
        ".#++#.",
        "#+..+#",
    ]),
    ('~', 7, &[
        ".#+..+",
        "#.+##+",
        "+....#",
    ]),
    ('`', 2, &[
        "#+..",
        ".+#.",
    ]),
    ('°', 2, &[
        ".##.",
        "#..#",
        "#..#",
        ".##.",
    ]),
    ('•', 7, &[
        ".##.",
        "####",
        "####",
        ".##.",
    ]),
    ('›', 5, &[
        "#+..",
        ".##.",
        "..##",
        "..##",
        ".##.",
        "#+..",
    ]),
    ('‹', 5, &[
        "..+#",
        ".##.",
        "##..",
        "##..",
        ".##.",
        "..+#",
    ]),
    ('«', 5, &[
        "..#..#",
        ".##.##",
        "##.##.",
        "##.##.",
        ".##.##",
        "..#..#",
    ]),
    ('»', 5, &[
        "#..#..",
        "##.##.",
        ".##.##",
        ".##.##",
        "##.##.",
        "#..#..",
    ]),
    ('—', 8, &[
        "########",
        "########",
    ]),
    ('’', 2, &[
        "##",
        "##",
        "#+",
    ]),
    ('…', 10, &[
        "##.##.##",
        "##.##.##",
    ]),
    ('↑', 3, &[
        "..##..",
        ".####.",
        "##++##",
        "#.##.#",
        "..##..",
        "..##..",
        "..##..",
        "..##..",
        "..##..",
    ]),
    ('↓', 3, &[
        "..##..",
        "..##..",
        "..##..",
        "..##..",
        "..##..",
        "#.##.#",
        "##++##",
        ".####.",
        "..##..",
    ]),
    ('→', 5, &[
        "..#...",
        "..##..",
        "######",
        "..##..",
        "..#...",
    ]),
    ('←', 5, &[
        "...#..",
        "..##..",
        "######",
        "..##..",
        "...#..",
    ]),
    // Panneau d'avertissement : triangle plein dont le point d'exclamation est
    // réservé en creux, comme sur un panneau routier. Un triangle en contour
    // avec une marque intérieure serait illisible au corps compact, où le trait
    // ne fait qu'un pixel : les deux corps adoptent donc le même parti.
    ('⚠', 4, &[
        "....#....",
        "...#.#...",
        "...#.#...",
        "..##.##..",
        "..##.##..",
        ".#######.",
        ".###.###.",
        "#########",
    ]),
];

/// Dessin servi pour tout caractère inconnu.
///
/// Un rectangle creux : il signale visiblement l'absence de glyphe sans
/// ressembler à une lettre, contrairement au « ? » utilisé auparavant qui
/// laissait croire à une ponctuation présente dans le texte.
#[rustfmt::skip]
const MEDIUM_FALLBACK: &[&str] = &[
    "#####",
    "#...#",
    "#...#",
    "#...#",
    "#...#",
    "#...#",
    "#...#",
    "#...#",
    "#...#",
    "#####",
];

/// Ordonnée de la première ligne du glyphe de repli.
const MEDIUM_FALLBACK_TOP: i32 = 2;

/// Convertit un caractère de dessin en couverture.
const fn coverage_of(ch: char) -> u8 {
    match ch {
        '#' => 255,
        '+' => 170,
        ':' => 90,
        _ => 0,
    }
}

/// Convertit un dessin en glyphe indexé.
fn parse(top: i32, rows: &[&str]) -> Glyph {
    let width = rows
        .iter()
        .map(|row| row.chars().count())
        .max()
        .unwrap_or(0) as i32;
    let mut coverage = Vec::with_capacity((width.max(0) as usize) * rows.len());

    for row in rows {
        let mut written = 0_i32;
        for ch in row.chars() {
            coverage.push(coverage_of(ch));
            written += 1;
        }
        // Les lignes plus courtes que la plus large sont complétées à vide :
        // le dessinateur n'a pas à aligner manuellement toutes ses lignes.
        let padding = (width - written).max(0) as usize;
        coverage.resize(coverage.len() + padding, 0);
    }

    Glyph {
        width,
        top,
        coverage,
    }
}

/// Superpose une marque diacritique à un glyphe de base.
///
/// La marque est centrée horizontalement sur la lettre, et posée à la hauteur
/// qu'impose sa nature et la casse de la base.
fn compose(
    base: &Glyph,
    mark_rows: &[&str],
    kind: MarkKind,
    uppercase: bool,
    geometry: &FaceGeometry,
) -> Glyph {
    let mark = parse(0, mark_rows);
    let mark_top = match kind {
        MarkKind::Below => geometry.below,
        MarkKind::Above if uppercase => geometry.uppercase,
        MarkKind::Above => geometry.lowercase,
    };

    let width = base.width.max(mark.width);
    let offset_x = (width - mark.width) / 2;
    let mark_rows_count = if mark.width > 0 {
        mark.coverage.len() as i32 / mark.width
    } else {
        0
    };

    let top = base.top.min(mark_top);
    let base_rows = if base.width > 0 {
        base.coverage.len() as i32 / base.width
    } else {
        0
    };
    let bottom = (base.top + base_rows).max(mark_top + mark_rows_count);
    let rows = (bottom - top).max(0);

    let mut coverage = vec![0_u8; (width.max(0) as usize) * (rows.max(0) as usize)];
    for row in 0..rows {
        for col in 0..width {
            let absolute_y = top + row;
            let from_base = base.coverage_at(col, absolute_y);
            let from_mark = mark.coverage_at(col - offset_x, absolute_y - mark_top);
            let index = (row * width + col) as usize;
            if let Some(slot) = coverage.get_mut(index) {
                *slot = from_base.max(from_mark);
            }
        }
    }

    Glyph {
        width,
        top,
        coverage,
    }
}

/// Construit tous les corps dessinés.
///
/// Le corps 11×20 reste à tracer ; le module parent sert le corps moyen à sa
/// place, ce qui donne un texte un peu plus petit que la maquette ne le prévoit
/// aux densités élevées, sans jamais rien casser.
pub(super) fn build_faces() -> HashMap<FontSize, Face> {
    let mut faces = HashMap::new();
    faces.insert(FontSize::Small, build_small());
    faces.insert(FontSize::Medium, build_medium());
    faces
}

/// Corps vide, utilisé comme garde-fou plutôt que de paniquer.
pub(super) fn empty_face() -> Face {
    Face {
        glyphs: HashMap::new(),
        fallback: parse(MEDIUM_FALLBACK_TOP, MEDIUM_FALLBACK),
        space: space_glyph(),
    }
}

/// Glyphe de l'espace : aucune encre, une avance.
fn space_glyph() -> Glyph {
    Glyph {
        width: SPACE_WIDTH,
        top: 0,
        coverage: Vec::new(),
    }
}

/// Assemble un corps : dessins directs, puis compositions, puis replis.
///
/// L'ordre compte : une composition a besoin de sa lettre de base, et un repli a
/// besoin de sa cible, composée comprise.
fn build_face(
    groups: &[&[Drawing]],
    geometry: &FaceGeometry,
    fallback: (i32, &[&'static str]),
) -> Face {
    let mut glyphs = HashMap::new();

    for (ch, top, rows) in groups.iter().copied().flatten() {
        glyphs.insert(*ch, parse(*top, rows));
    }

    for (accented, base, mark_name) in COMPOSITIONS {
        let Some(base_glyph) = glyphs.get(base) else {
            continue;
        };
        let Some((_, kind, rows)) = MARKS.iter().find(|(name, _, _)| name == mark_name) else {
            continue;
        };

        let composed = compose(base_glyph, rows, *kind, base.is_uppercase(), geometry);
        glyphs.insert(*accented, composed);
    }

    for (from, to) in FOLDS {
        if let Some(target) = glyphs.get(to).cloned() {
            glyphs.entry(*from).or_insert(target);
        }
    }

    Face {
        glyphs,
        fallback: parse(fallback.0, fallback.1),
        space: space_glyph(),
    }
}

/// Assemble le corps moyen 8×15.
fn build_medium() -> Face {
    build_face(
        &[
            MEDIUM_UPPERCASE,
            MEDIUM_LOWERCASE,
            MEDIUM_DIGITS,
            MEDIUM_SYMBOLS,
        ],
        &MEDIUM_GEOMETRY,
        (MEDIUM_FALLBACK_TOP, MEDIUM_FALLBACK),
    )
}

/// Assemble le corps compact 6×11.
fn build_small() -> Face {
    build_face(
        &[
            small::UPPERCASE,
            small::LOWERCASE,
            small::DIGITS,
            small::SYMBOLS,
        ],
        &SMALL_GEOMETRY,
        (small::FALLBACK_TOP, small::FALLBACK),
    )
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_every_drawing_stays_inside_its_cell() {
        // Un dessin qui dépasse la cellule empiéterait sur la ligne voisine.
        //
        // Les deux corps sont contrôlés. Le corps compact ne l'était pas : un
        // glyphe deux fois trop large y serait passé sans que rien ne le signale,
        // et n'aurait été vu qu'en regardant la planche — si tant est qu'on ait
        // pensé à l'y faire figurer.
        for (face, groups, max_width) in [
            (
                FontSize::Medium,
                [
                    MEDIUM_UPPERCASE,
                    MEDIUM_LOWERCASE,
                    MEDIUM_DIGITS,
                    MEDIUM_SYMBOLS,
                ],
                9,
            ),
            (
                FontSize::Small,
                [
                    small::UPPERCASE,
                    small::LOWERCASE,
                    small::DIGITS,
                    small::SYMBOLS,
                ],
                8,
            ),
        ] {
            let height = face.cell_height();
            for (ch, top, rows) in groups.iter().copied().flatten() {
                let bottom = top + rows.len() as i32;
                assert!(
                    *top >= 0 && bottom <= height,
                    "« {ch} » sort de la cellule {face:?} : lignes {top}..{bottom} pour une hauteur de {height}"
                );
                assert!(
                    rows.iter().all(|row| row.chars().count() <= max_width),
                    "« {ch} » est plus large que la cellule {face:?} autorisée ({max_width} px)"
                );
            }
        }
    }

    #[test]
    fn test_drawings_use_only_the_documented_coverage_levels() {
        for (ch, _, rows) in MEDIUM_UPPERCASE
            .iter()
            .chain(MEDIUM_LOWERCASE)
            .chain(MEDIUM_DIGITS)
            .chain(MEDIUM_SYMBOLS)
        {
            for row in *rows {
                for c in row.chars() {
                    assert!(
                        matches!(c, '.' | ':' | '+' | '#'),
                        "« {ch} » emploie le caractère de dessin inconnu « {c} »"
                    );
                }
            }
        }
    }

    #[test]
    fn test_no_glyph_is_drawn_twice() {
        let mut seen = std::collections::HashSet::new();
        for (ch, _, _) in MEDIUM_UPPERCASE
            .iter()
            .chain(MEDIUM_LOWERCASE)
            .chain(MEDIUM_DIGITS)
            .chain(MEDIUM_SYMBOLS)
        {
            assert!(seen.insert(*ch), "« {ch} » est dessiné deux fois");
        }
    }

    #[test]
    fn test_the_printable_ascii_range_is_fully_covered() {
        // Le panneau affiche des chemins, des messages de commit et des
        // libellés : un trou dans l'ASCII imprimable serait immédiatement
        // visible à l'écran.
        let face = build_medium();
        let mut missing = Vec::new();
        for code in 0x21_u8..=0x7e {
            let ch = char::from(code);
            if !face.glyphs.contains_key(&ch) {
                missing.push(ch);
            }
        }
        assert!(missing.is_empty(), "glyphes ASCII manquants : {missing:?}");
    }

    #[test]
    fn test_every_composition_resolves() {
        let face = build_medium();
        for (accented, base, mark) in COMPOSITIONS {
            assert!(
                face.glyphs.contains_key(accented),
                "la composition « {accented} » = « {base} » + {mark} n'a pas abouti"
            );
        }
    }

    #[test]
    fn test_composition_never_erases_the_base_letter() {
        let face = build_medium();
        for (accented, base, _) in COMPOSITIONS {
            let composed = face.glyphs.get(accented).expect("glyphe composé");
            let plain = face.glyphs.get(base).expect("glyphe de base");

            let composed_ink: u32 = composed.coverage.iter().map(|&c| u32::from(c)).sum();
            let plain_ink: u32 = plain.coverage.iter().map(|&c| u32::from(c)).sum();
            assert!(
                composed_ink > plain_ink,
                "« {accented} » n'ajoute rien à « {base} »"
            );
        }
    }

    #[test]
    fn test_short_rows_are_padded_not_misaligned() {
        // Le dessinateur peut laisser une ligne plus courte : elle doit être
        // complétée à droite, sans décaler les pixels suivants.
        let glyph = parse(0, &["##", "#"]);
        assert_eq!(glyph.width, 2);
        assert_eq!(glyph.coverage, vec![255, 255, 255, 0]);
    }

    #[test]
    fn test_parsing_tolerates_degenerate_drawings() {
        let empty = parse(0, &[]);
        assert_eq!(empty.width, 0);
        assert_eq!(empty.coverage_at(0, 0), 0);

        let blank = parse(0, &["", ""]);
        assert_eq!(blank.width, 0);
    }
}
