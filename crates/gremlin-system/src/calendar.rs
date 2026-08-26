//! Façade de calendrier local : la seule source du « jour courant ».
//!
//! `gremlin-core` ne lit aucune horloge — c'est ce qui rend ses séries de
//! productivité testables sans dormir ni bricoler l'heure de la machine. Le jour
//! civil lui est donc **injecté**, et il vient d'ici.
//!
//! ## Pourquoi une dépendance de dates
//!
//! Obtenir « la date locale d'aujourd'hui » suppose de connaître le décalage UTC
//! en vigueur *à cet instant précis*, heure d'été comprise. Cette information vit
//! dans la base de fuseaux IANA (ou son équivalent registre sous Windows), dont
//! les règles changent plusieurs fois par an par décision politique. La
//! réimplémenter reviendrait à embarquer une copie de la base et à la maintenir ;
//! une approximation, elle, décalerait les séries d'une journée entière deux fois
//! par an. [`jiff`] est donc utilisé, confiné à ce module.
//!
//! La conversion inverse — l'horodatage d'un commit vers sa date civile — n'a
//! **pas** besoin de cette base : Git enregistre le décalage avec le commit, et
//! `gremlin-core` fait l'arithmétique lui-même.

use crate::error::SystemError;
use jiff::{Timestamp, Unit, Zoned};

/// Composants d'une date civile locale, sans heure ni fuseau.
///
/// Type de transport volontairement minimal : `gremlin-app` le convertit
/// explicitement vers la `CivilDate` du domaine, qui, elle, valide le calendrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalDateParts {
    /// Année civile.
    pub year: i32,
    /// Mois civil, dans `1..=12`.
    pub month: u8,
    /// Jour du mois, dans `1..=31`.
    pub day: u8,
}

/// Source injectable du jour civil courant.
///
/// Le trait existe pour que les tests fournissent un calendrier figé plutôt que
/// de modifier l'heure ou le fuseau de la machine — ce qu'aucun test n'a le droit
/// de faire.
pub trait LocalCalendar: Send + Sync {
    /// Date civile locale d'aujourd'hui.
    ///
    /// # Errors
    /// Renvoie [`SystemError::CalendarUnavailable`] si le fuseau du système est
    /// introuvable ou si l'horloge renvoie un instant non représentable. Aucun
    /// repli silencieux sur UTC : une date fausse est pire qu'une date absente.
    fn today(&self) -> Result<LocalDateParts, SystemError>;

    /// Secondes restantes avant le prochain minuit local.
    ///
    /// Sert à programmer le rafraîchissement de la série au changement de jour.
    /// La valeur n'est pas toujours 86 400 moins l'heure courante : un passage à
    /// l'heure d'été raccourcit ou rallonge la journée.
    ///
    /// # Errors
    /// Mêmes conditions que [`Self::today`].
    fn seconds_until_next_midnight(&self) -> Result<u64, SystemError>;
}

/// Calendrier adossé à l'horloge et au fuseau du système hôte.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemLocalCalendar;

impl SystemLocalCalendar {
    /// Construit la façade système.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Instant local courant, ou une erreur explicite.
    ///
    /// Fonction associée : la façade est sans état, l'horloge et le fuseau
    /// viennent du système à chaque appel.
    fn now() -> Result<Zoned, SystemError> {
        let zone = jiff::tz::TimeZone::try_system()
            .map_err(|error| SystemError::CalendarUnavailable(error.to_string()))?;
        Ok(Timestamp::now().to_zoned(zone))
    }
}

impl LocalCalendar for SystemLocalCalendar {
    fn today(&self) -> Result<LocalDateParts, SystemError> {
        let now = Self::now()?;
        let date = now.date();
        Ok(LocalDateParts {
            year: i32::from(date.year()),
            // `jiff` garantit `1..=12` et `1..=31` : la conversion ne tronque pas.
            month: date.month() as u8,
            day: date.day() as u8,
        })
    }

    fn seconds_until_next_midnight(&self) -> Result<u64, SystemError> {
        let now = Self::now()?;
        let next_midnight = now
            .tomorrow()
            .and_then(|day| day.start_of_day())
            .map_err(|error| SystemError::CalendarUnavailable(error.to_string()))?;

        let span = next_midnight
            .since((Unit::Second, &now))
            .map_err(|error| SystemError::CalendarUnavailable(error.to_string()))?;

        // Un écart négatif signalerait une horloge qui recule pendant le calcul :
        // on ne programme alors aucune attente plutôt qu'une attente absurde.
        Ok(u64::try_from(span.get_seconds()).unwrap_or(0))
    }
}

/// Calendrier figé, destiné aux tests et aux scénarios reproductibles.
///
/// Il évite d'avoir à toucher à l'horloge ou au fuseau de la machine pour
/// éprouver les règles de série autour de minuit, des fins de mois ou du 29
/// février.
#[derive(Debug, Clone, Copy)]
pub struct FixedCalendar {
    /// Date renvoyée par [`LocalCalendar::today`].
    pub date: LocalDateParts,
    /// Valeur renvoyée par [`LocalCalendar::seconds_until_next_midnight`].
    pub seconds_until_midnight: u64,
}

impl FixedCalendar {
    /// Construit un calendrier figé sur la date donnée.
    #[must_use]
    pub const fn new(year: i32, month: u8, day: u8) -> Self {
        Self {
            date: LocalDateParts { year, month, day },
            seconds_until_midnight: 3_600,
        }
    }
}

impl LocalCalendar for FixedCalendar {
    fn today(&self) -> Result<LocalDateParts, SystemError> {
        Ok(self.date)
    }

    fn seconds_until_next_midnight(&self) -> Result<u64, SystemError> {
        Ok(self.seconds_until_midnight)
    }
}

/// Calendrier qui échoue systématiquement, pour éprouver les chemins d'erreur.
///
/// Une date locale indisponible ne doit ni corrompre la sauvegarde, ni être
/// remplacée par une valeur inventée : ce double sert à le vérifier.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableCalendar;

impl LocalCalendar for UnavailableCalendar {
    fn today(&self) -> Result<LocalDateParts, SystemError> {
        Err(SystemError::CalendarUnavailable(String::from(
            "calendrier indisponible",
        )))
    }

    fn seconds_until_next_midnight(&self) -> Result<u64, SystemError> {
        Err(SystemError::CalendarUnavailable(String::from(
            "calendrier indisponible",
        )))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{
        FixedCalendar, LocalCalendar, LocalDateParts, SystemLocalCalendar, UnavailableCalendar,
    };
    use crate::error::SystemError;

    #[test]
    fn test_system_calendar_returns_a_plausible_date() {
        // Le test n'affirme pas *quelle* date : il vérifie que la façade répond
        // et que ses composants sont dans le calendrier. Toucher au fuseau de la
        // machine serait un effet de bord interdit à un test.
        let Ok(today) = SystemLocalCalendar::new().today() else {
            panic!("la date locale doit être disponible sur une machine de test");
        };
        assert!(today.year >= 2024 && today.year <= 9_999);
        assert!((1..=12).contains(&today.month));
        assert!((1..=31).contains(&today.day));
    }

    #[test]
    fn test_seconds_until_midnight_stay_within_a_long_day() {
        let Ok(seconds) = SystemLocalCalendar::new().seconds_until_next_midnight() else {
            panic!("le prochain minuit doit être calculable");
        };
        // Une journée peut durer 23 h ou 25 h lors d'un changement d'heure ; au-delà,
        // le calcul serait faux.
        assert!(seconds <= 25 * 3_600, "attente absurde : {seconds} s");
    }

    #[test]
    fn test_fixed_calendar_is_stable() {
        let calendar = FixedCalendar::new(2024, 2, 29);
        assert_eq!(
            calendar.today().unwrap(),
            LocalDateParts {
                year: 2024,
                month: 2,
                day: 29,
            }
        );
        assert_eq!(calendar.today().unwrap(), calendar.today().unwrap());
    }

    #[test]
    fn test_unavailable_calendar_reports_an_error_instead_of_a_date() {
        let calendar = UnavailableCalendar;
        assert!(matches!(
            calendar.today(),
            Err(SystemError::CalendarUnavailable(_))
        ));
        assert!(matches!(
            calendar.seconds_until_next_midnight(),
            Err(SystemError::CalendarUnavailable(_))
        ));
    }
}
