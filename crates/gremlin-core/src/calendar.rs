//! Dates civiles locales du domaine, sans horloge ni fuseau horaire.
//!
//! `gremlin-core` ne lit jamais l'heure : le jour courant lui est **injecté**
//! par l'orchestrateur, qui l'obtient de la façade calendrier de
//! `gremlin-system`. Ce module fournit le seul type de date que le domaine
//! manipule, avec deux garanties que l'appelant n'a pas à revérifier :
//!
//! * une [`CivilDate`] construite existe réellement dans le calendrier
//!   grégorien proleptique et tombe dans la fenêtre supportée ;
//! * l'écart en jours entre deux dates est un calcul entier exact, sans
//!   flottant ni arithmétique naïve sur un `YYYYMMDD`.
//!
//! La représentation interne est le **numéro de jour** depuis le 1er janvier
//! 1970. C'est une bijection avec le triplet (année, mois, jour) : l'ordre
//! naturel, l'égalité et la consécutivité s'y expriment sans cas particulier de
//! fin de mois ni d'année bissextile.

use crate::error::CoreError;

/// Première année acceptée par le domaine (époque Unix).
///
/// Un horodatage Git antérieur à l'époque est une donnée corrompue, pas une
/// journée de travail : le domaine la refuse au lieu de la projeter.
pub const MIN_CIVIL_YEAR: i32 = 1970;

/// Dernière année acceptée par le domaine.
///
/// La borne haute protège des horodatages absurdes lus sur le disque : sans
/// elle, une date à cinq chiffres traverserait le calcul de série.
pub const MAX_CIVIL_YEAR: i32 = 9_999;

/// Numéro du jour le plus ancien accepté (1970-01-01).
pub const MIN_DAY_NUMBER: i32 = days_from_civil(MIN_CIVIL_YEAR, 1, 1);

/// Numéro du jour le plus récent accepté (9999-12-31).
pub const MAX_DAY_NUMBER: i32 = days_from_civil(MAX_CIVIL_YEAR, 12, 31);

/// Nombre de secondes dans une journée civile nominale.
pub const SECONDS_PER_DAY: i64 = 86_400;

/// Une date civile locale, sans heure ni fuseau.
///
/// Le type est `Copy` et totalement ordonné : deux dates se comparent et se
/// soustraient directement. Ses champs sont privés — une date ne peut pas être
/// fabriquée hors du calendrier grégorien ni hors de la fenêtre supportée.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CivilDate {
    /// Jours écoulés depuis le 1er janvier 1970, valeur toujours bornée.
    day_number: i32,
}

impl CivilDate {
    /// Construit une date à partir d'un triplet civil.
    ///
    /// # Errors
    /// Renvoie [`CoreError::InvalidCivilDate`] si le mois est hors `1..=12`, si
    /// le jour n'existe pas dans ce mois (29 février d'une année commune, 31
    /// avril...) ou si l'année sort de `[MIN_CIVIL_YEAR, MAX_CIVIL_YEAR]`.
    pub const fn new(year: i32, month: u8, day: u8) -> Result<Self, CoreError> {
        if year < MIN_CIVIL_YEAR || year > MAX_CIVIL_YEAR || month < 1 || month > 12 || day < 1 {
            return Err(CoreError::InvalidCivilDate { year, month, day });
        }
        if day > days_in_month(year, month) {
            return Err(CoreError::InvalidCivilDate { year, month, day });
        }
        Ok(Self {
            day_number: days_from_civil(year, month, day),
        })
    }

    /// Construit une date à partir d'un numéro de jour depuis l'époque Unix.
    ///
    /// C'est le point d'entrée des données persistées : la sauvegarde ne stocke
    /// que des entiers, et toute valeur hors fenêtre est refusée ici plutôt que
    /// propagée dans le calcul de série.
    ///
    /// # Errors
    /// Renvoie [`CoreError::CivilDayOutOfRange`] si le numéro sort de
    /// `[MIN_DAY_NUMBER, MAX_DAY_NUMBER]`.
    pub const fn from_day_number(day_number: i32) -> Result<Self, CoreError> {
        if day_number < MIN_DAY_NUMBER || day_number > MAX_DAY_NUMBER {
            return Err(CoreError::CivilDayOutOfRange { day_number });
        }
        Ok(Self { day_number })
    }

    /// Construit la date locale portée par un horodatage Unix et son décalage.
    ///
    /// Le décalage est celui enregistré par Git **au moment du commit** : c'est
    /// lui qui fait foi, pas le fuseau courant de la machine. Changer de fuseau
    /// ne réécrit donc jamais les journées historiques.
    ///
    /// # Errors
    /// Renvoie [`CoreError::CivilDayOutOfRange`] si l'instant local tombe hors
    /// de la fenêtre supportée, y compris après débordement du décalage.
    pub const fn from_unix_seconds(
        unix_seconds: i64,
        utc_offset_minutes: i16,
    ) -> Result<Self, CoreError> {
        let Some(local) = unix_seconds.checked_add(utc_offset_minutes as i64 * 60) else {
            return Err(CoreError::CivilDayOutOfRange {
                day_number: i32::MAX,
            });
        };
        // Division euclidienne : `-1 / 86_400` vaut 0 en Rust, ce qui placerait
        // le 31 décembre 1969 au 1er janvier 1970. `div_euclid` renvoie -1.
        let day = local.div_euclid(SECONDS_PER_DAY);
        if day < MIN_DAY_NUMBER as i64 || day > MAX_DAY_NUMBER as i64 {
            return Err(CoreError::CivilDayOutOfRange {
                day_number: if day > i32::MAX as i64 {
                    i32::MAX
                } else if day < i32::MIN as i64 {
                    i32::MIN
                } else {
                    day as i32
                },
            });
        }
        Ok(Self {
            day_number: day as i32,
        })
    }

    /// Numéro du jour depuis le 1er janvier 1970.
    ///
    /// C'est la forme persistée : compacte, ordonnable et sans cas particulier
    /// de calendrier.
    #[must_use]
    pub const fn day_number(self) -> i32 {
        self.day_number
    }

    /// Année civile.
    #[must_use]
    pub const fn year(self) -> i32 {
        civil_from_days(self.day_number).0
    }

    /// Mois civil, dans `1..=12`.
    #[must_use]
    pub const fn month(self) -> u8 {
        civil_from_days(self.day_number).1
    }

    /// Jour du mois, dans `1..=31`.
    #[must_use]
    pub const fn day(self) -> u8 {
        civil_from_days(self.day_number).2
    }

    /// Nombre de jours séparant `earlier` de `self`, négatif si `self` précède.
    ///
    /// Le résultat tient toujours dans un `i32` : les deux opérandes sont
    /// bornées par construction, leur écart maximal vaut donc
    /// `MAX_DAY_NUMBER - MIN_DAY_NUMBER`.
    #[must_use]
    pub const fn days_since(self, earlier: Self) -> i32 {
        self.day_number - earlier.day_number
    }

    /// Lendemain de cette date, ou `None` au bord haut de la fenêtre.
    #[must_use]
    pub const fn next_day(self) -> Option<Self> {
        self.checked_add_days(1)
    }

    /// Date décalée de `days` jours, ou `None` si le résultat sort de la fenêtre.
    #[must_use]
    pub const fn checked_add_days(self, days: i32) -> Option<Self> {
        let Some(shifted) = self.day_number.checked_add(days) else {
            return None;
        };
        if shifted < MIN_DAY_NUMBER || shifted > MAX_DAY_NUMBER {
            return None;
        }
        Some(Self {
            day_number: shifted,
        })
    }
}

impl core::fmt::Display for CivilDate {
    /// Format ISO 8601 `YYYY-MM-DD`, seul format affiché par l'interface.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (year, month, day) = civil_from_days(self.day_number);
        write!(f, "{year:04}-{month:02}-{day:02}")
    }
}

/// Indique si une année est bissextile dans le calendrier grégorien.
#[must_use]
pub const fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Nombre de jours du mois donné, en tenant compte des années bissextiles.
///
/// Un mois hors `1..=12` renvoie 0 : aucun jour ne peut alors être validé.
#[must_use]
pub const fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Convertit un triplet civil en numéro de jour depuis l'époque Unix.
///
/// Algorithme « days from civil » de Howard Hinnant : exact en arithmétique
/// entière sur tout le calendrier grégorien proleptique, sans table ni boucle.
/// Le décalage `719_468` ramène l'origine interne (1er mars de l'an 0) sur le
/// 1er janvier 1970.
const fn days_from_civil(year: i32, month: u8, day: u8) -> i32 {
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let year_of_era = y - era * 400;
    let shifted_month = if month > 2 { month - 3 } else { month + 9 } as i64;
    let day_of_year = (153 * shifted_month + 2) / 5 + day as i64 - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    // Le résultat reste très en deçà des bornes `i32` pour toute année de la
    // fenêtre supportée : la conversion ne peut pas tronquer.
    (era * 146_097 + day_of_era - 719_468) as i32
}

/// Convertit un numéro de jour depuis l'époque Unix en triplet civil.
///
/// Inverse exact de [`days_from_civil`].
const fn civil_from_days(day_number: i32) -> (i32, u8, u8) {
    let z = day_number as i64 + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    // `month` tient dans `1..=12` et `day` dans `1..=31` par construction de
    // l'algorithme ; `year` reste dans la fenêtre supportée.
    (year as i32, month as u8, day as u8)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_epoch_is_day_zero() {
        let epoch = CivilDate::new(1970, 1, 1).unwrap();
        assert_eq!(epoch.day_number(), 0);
        assert_eq!(epoch.year(), 1970);
        assert_eq!(epoch.month(), 1);
        assert_eq!(epoch.day(), 1);
    }

    #[test]
    fn test_roundtrip_over_a_long_span() {
        // Un aller-retour sur plusieurs décennies vérifie l'inverse exact, pas
        // seulement quelques dates choisies.
        let mut day = MIN_DAY_NUMBER;
        while day <= MIN_DAY_NUMBER + 40_000 {
            let date = CivilDate::from_day_number(day).unwrap();
            let rebuilt = CivilDate::new(date.year(), date.month(), date.day()).unwrap();
            assert_eq!(rebuilt.day_number(), day, "aller-retour rompu à {day}");
            day += 1;
        }
    }

    #[test]
    fn test_leap_day_accepted_only_on_leap_years() {
        assert!(CivilDate::new(2024, 2, 29).is_ok());
        assert!(CivilDate::new(2000, 2, 29).is_ok());
        assert!(CivilDate::new(2023, 2, 29).is_err());
        assert!(CivilDate::new(1900, 2, 29).is_err());
        assert!(CivilDate::new(2100, 2, 29).is_err());
    }

    #[test]
    fn test_invalid_triplets_are_refused() {
        assert!(CivilDate::new(2024, 0, 10).is_err());
        assert!(CivilDate::new(2024, 13, 10).is_err());
        assert!(CivilDate::new(2024, 4, 31).is_err());
        assert!(CivilDate::new(2024, 1, 0).is_err());
        assert!(CivilDate::new(1969, 12, 31).is_err());
        assert!(CivilDate::new(10_000, 1, 1).is_err());
    }

    #[test]
    fn test_day_number_bounds_are_refused_outside_the_window() {
        assert!(CivilDate::from_day_number(MIN_DAY_NUMBER).is_ok());
        assert!(CivilDate::from_day_number(MAX_DAY_NUMBER).is_ok());
        assert!(CivilDate::from_day_number(MIN_DAY_NUMBER - 1).is_err());
        assert!(CivilDate::from_day_number(MAX_DAY_NUMBER + 1).is_err());
        assert!(CivilDate::from_day_number(i32::MIN).is_err());
        assert!(CivilDate::from_day_number(i32::MAX).is_err());
    }

    #[test]
    fn test_month_and_year_boundaries_are_consecutive() {
        let end_of_month = CivilDate::new(2024, 1, 31).unwrap();
        assert_eq!(
            end_of_month.next_day().unwrap(),
            CivilDate::new(2024, 2, 1).unwrap()
        );

        let end_of_year = CivilDate::new(2023, 12, 31).unwrap();
        assert_eq!(
            end_of_year.next_day().unwrap(),
            CivilDate::new(2024, 1, 1).unwrap()
        );

        let leap_day = CivilDate::new(2024, 2, 29).unwrap();
        assert_eq!(
            leap_day.next_day().unwrap(),
            CivilDate::new(2024, 3, 1).unwrap()
        );
    }

    #[test]
    fn test_days_since_is_signed_and_exact() {
        let a = CivilDate::new(2024, 1, 1).unwrap();
        let b = CivilDate::new(2024, 3, 1).unwrap();
        // 2024 est bissextile : janvier (31) + février (29).
        assert_eq!(b.days_since(a), 60);
        assert_eq!(a.days_since(b), -60);
        assert_eq!(a.days_since(a), 0);

        let c = CivilDate::new(2023, 1, 1).unwrap();
        let d = CivilDate::new(2024, 1, 1).unwrap();
        assert_eq!(d.days_since(c), 365);
    }

    #[test]
    fn test_unix_seconds_use_the_recorded_offset() {
        // 2024-01-01T23:30:00Z. À UTC+02:00 c'est déjà le 2 janvier local ;
        // à UTC-05:00 on est encore le 1er.
        let utc_instant = 1_704_151_800;
        let plus_two = CivilDate::from_unix_seconds(utc_instant, 120).unwrap();
        let minus_five = CivilDate::from_unix_seconds(utc_instant, -300).unwrap();

        assert_eq!(plus_two, CivilDate::new(2024, 1, 2).unwrap());
        assert_eq!(minus_five, CivilDate::new(2024, 1, 1).unwrap());
    }

    #[test]
    fn test_unix_seconds_before_epoch_are_refused() {
        assert!(CivilDate::from_unix_seconds(-1, 0).is_err());
        assert!(CivilDate::from_unix_seconds(0, -60).is_err());
        assert!(CivilDate::from_unix_seconds(i64::MIN, 0).is_err());
    }

    #[test]
    fn test_hostile_timestamps_do_not_overflow() {
        // Un horodatage démesuré et un décalage extrême ne doivent produire ni
        // panique ni date fabriquée.
        assert!(CivilDate::from_unix_seconds(i64::MAX, i16::MAX).is_err());
        assert!(CivilDate::from_unix_seconds(i64::MAX, i16::MIN).is_err());
        assert!(CivilDate::from_unix_seconds(i64::MIN, i16::MAX).is_err());
    }

    #[test]
    fn test_checked_add_days_stops_at_the_window_edges() {
        let last = CivilDate::from_day_number(MAX_DAY_NUMBER).unwrap();
        assert!(last.next_day().is_none());
        assert!(last.checked_add_days(i32::MAX).is_none());

        let first = CivilDate::from_day_number(MIN_DAY_NUMBER).unwrap();
        assert!(first.checked_add_days(-1).is_none());
        assert!(first.checked_add_days(i32::MIN).is_none());
    }

    #[test]
    fn test_ordering_follows_the_calendar() {
        let mut dates = [
            CivilDate::new(2024, 3, 1).unwrap(),
            CivilDate::new(2023, 12, 31).unwrap(),
            CivilDate::new(2024, 1, 15).unwrap(),
        ];
        dates.sort_unstable();
        assert_eq!(dates[0], CivilDate::new(2023, 12, 31).unwrap());
        assert_eq!(dates[1], CivilDate::new(2024, 1, 15).unwrap());
        assert_eq!(dates[2], CivilDate::new(2024, 3, 1).unwrap());
    }

    #[test]
    fn test_display_is_iso_8601() {
        let date = CivilDate::new(2024, 2, 9).unwrap();
        assert_eq!(date.to_string(), "2024-02-09");
    }
}
