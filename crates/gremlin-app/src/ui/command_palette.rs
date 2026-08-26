//! Moteur de recherche et logique de la palette de commande style Raycast.

use crate::config::AppConfig;
use crate::ui::search;
use gremlin_core::{
    CivilDate, ConsumableKind, PetState, PomodoroPhase, PomodoroState, StreakReward,
};
use gremlin_render::{AccessoryCatalog, AccessoryCategory, AccessorySource, WardrobeEquipment};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

/// Section logique regroupant les éléments dans la liste.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PaletteSection {
    /// Profil et jauges vitales du Gremlin.
    PetProfile,
    /// Soins et interactions directes avec le familier.
    PetCare,
    /// Accessoires et éléments actuellement équipés.
    ActiveEquipment,
    /// Chapeaux et couvre-chefs.
    Hats,
    /// Lunettes et visières.
    Glasses,
    /// Sweats et tenues.
    Outfits,
    /// Objets tenus en main.
    Held,
    /// Auras et effets spéciaux.
    Auras,
    /// Surveillance des dépôts Git.
    GitWatcher,
    /// Séries de jours de commits et récompenses associées.
    Streak,
    /// Consommables détenus.
    Inventory,
    /// Minuteur de concentration.
    FocusTimer,
    /// Placement du familier sur le bureau.
    DesktopBehaviour,
    /// Préférences générales et système.
    GeneralSettings,
}

impl PaletteSection {
    /// Titre affiché de la section en lettres capitales.
    #[must_use]
    pub const fn header_title(self) -> &'static str {
        match self {
            Self::PetProfile => "PROFIL GREMLIN",
            Self::PetCare => "SOINS & ACTIONS",
            Self::ActiveEquipment => "ACTIF",
            Self::Hats => "CHAPEAUX",
            Self::Glasses => "LUNETTES",
            Self::Outfits => "TENUES",
            Self::Held => "OBJETS TENUS",
            Self::Auras => "AURAS",
            Self::GitWatcher => "SURVEILLANCE GIT",
            Self::Streak => "SÉRIE DE COMMITS",
            Self::Inventory => "INVENTAIRE",
            Self::FocusTimer => "CONCENTRATION",
            Self::DesktopBehaviour => "PLACEMENT SUR LE BUREAU",
            Self::GeneralSettings => "PRÉFÉRENCES SYSTÈME",
        }
    }

    /// Groupe de premier niveau auquel appartient la section.
    ///
    /// Correspondance totale et unique : c'est elle qui rattache chaque feuille
    /// à son entrée de racine, sans qu'aucune section ne puisse être orpheline.
    #[must_use]
    pub const fn group(self) -> PaletteGroup {
        match self {
            Self::PetProfile => PaletteGroup::Profile,
            Self::PetCare => PaletteGroup::Care,
            Self::ActiveEquipment
            | Self::Hats
            | Self::Glasses
            | Self::Outfits
            | Self::Held
            | Self::Auras => PaletteGroup::Wardrobe,
            Self::GitWatcher => PaletteGroup::Repos,
            Self::Streak | Self::Inventory | Self::FocusTimer => PaletteGroup::Productivity,
            Self::DesktopBehaviour | Self::GeneralSettings => PaletteGroup::Preferences,
        }
    }
}

/// Groupe de premier niveau affiché à la racine du panneau.
///
/// La liste présentait une vingtaine d'items — plus un par dépôt Git détecté,
/// sans plafond, le scan ratissant le répertoire personnel sur cinq niveaux — à
/// plat sur neuf lignes visibles, sans en-tête ni ascenseur. Regrouper en cinq
/// entrées de racine rend la liste parcourable, et c'est le modèle de Raycast :
/// la racine énumère des commandes, chacune ouvrant sa propre liste, la
/// recherche restant globale et traversant tous les niveaux.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PaletteGroup {
    /// Profil, niveau et jauges vitales.
    Profile,
    /// Soins et interactions directes.
    Care,
    /// Garde-robe : toutes les catégories d'accessoires.
    Wardrobe,
    /// Dépôts Git surveillés.
    Repos,
    /// Séries, inventaire et minuteur de concentration.
    Productivity,
    /// Préférences système et actions de maintenance.
    Preferences,
}

impl PaletteGroup {
    /// Tous les groupes, dans l'ordre d'affichage à la racine.
    pub const ALL: [Self; 6] = [
        Self::Profile,
        Self::Care,
        Self::Wardrobe,
        Self::Repos,
        Self::Productivity,
        Self::Preferences,
    ];

    /// Titre affiché de l'entrée de racine.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Profile => "Profil du Gremlin",
            Self::Care => "Soins et actions",
            Self::Wardrobe => "Garde-robe",
            Self::Repos => "Dépôts surveillés",
            Self::Productivity => "Productivité",
            Self::Preferences => "Préférences système",
        }
    }

    /// Section représentative du groupe.
    ///
    /// Ne sert qu'à donner une valeur cohérente au champ `section` d'une entrée
    /// de racine ; le rendu n'affiche pas de libellé de section sur ces lignes,
    /// puisqu'elles *sont* les sections.
    #[must_use]
    pub const fn sections_head(self) -> PaletteSection {
        match self {
            Self::Profile => PaletteSection::PetProfile,
            Self::Care => PaletteSection::PetCare,
            Self::Wardrobe => PaletteSection::Hats,
            Self::Repos => PaletteSection::GitWatcher,
            Self::Productivity => PaletteSection::Streak,
            Self::Preferences => PaletteSection::GeneralSettings,
        }
    }

    /// Sous-titre expliquant ce que le groupe contient.
    #[must_use]
    pub const fn subtitle(self) -> &'static str {
        match self {
            Self::Profile => "Niveau, humeur, expérience et constantes vitales",
            Self::Care => "Nourrir, soigner, endormir, réanimer",
            Self::Wardrobe => "Chapeaux, lunettes, tenues, objets, auras",
            Self::Repos => "Branches et derniers commits détectés",
            Self::Productivity => "Série de commits, inventaire, concentration",
            Self::Preferences => "Démarrage, échelle, dossiers, sauvegarde",
        }
    }
}

/// Nature d'une saisie guidée occupant le champ de recherche.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// Chemin absolu du dépôt Git à confier à la surveillance.
    AddTrackedRepo,
}

impl PromptKind {
    /// Fil d'Ariane du mode saisie.
    #[must_use]
    pub const fn breadcrumb(self) -> &'static str {
        match self {
            Self::AddTrackedRepo => "Ajouter un dépôt",
        }
    }

    /// Consigne affichée quand la saisie est encore vide.
    #[must_use]
    pub const fn hint(self) -> &'static str {
        match self {
            Self::AddTrackedRepo => "Chemin absolu du dépôt, puis Entrée",
        }
    }

    /// Groupe auquel revenir en quittant la saisie.
    #[must_use]
    pub const fn parent_group(self) -> PaletteGroup {
        match self {
            Self::AddTrackedRepo => PaletteGroup::Repos,
        }
    }
}

/// Verdict de la validation d'un chemin saisi par l'utilisateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepoPathVerdict {
    /// Rien n'a encore été saisi.
    Empty,
    /// Chemin relatif : le répertoire de travail d'une application résidente
    /// n'est pas un point de départ fiable.
    Relative,
    /// Le dossier existe peut-être, mais ne porte pas de `.git`.
    NotARepo,
    /// Dépôt Git valide.
    Valid,
}

impl RepoPathVerdict {
    /// Analyse une saisie brute.
    fn of(raw: &str) -> Self {
        if raw.is_empty() {
            return Self::Empty;
        }
        let path = Path::new(raw);
        if !path.is_absolute() {
            return Self::Relative;
        }
        if gremlin_watcher::is_git_repo(path) {
            Self::Valid
        } else {
            Self::NotARepo
        }
    }

    /// Message montré à l'utilisateur pendant sa saisie.
    const fn message(self) -> &'static str {
        match self {
            Self::Empty => "Collez ou saisissez le chemin du dépôt",
            Self::Relative => "Chemin relatif : indiquez un chemin absolu",
            Self::NotARepo => "Ce dossier n'est pas un dépôt Git valide",
            Self::Valid => "Dépôt Git valide — Entrée pour confirmer",
        }
    }

    /// Pastille de la ligne de confirmation.
    const fn badge(self) -> &'static str {
        match self {
            Self::Empty => "EN ATTENTE",
            Self::Relative | Self::NotARepo => "INVALIDE",
            Self::Valid => "VALIDE",
        }
    }
}

/// Niveau de navigation courant du panneau.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PaletteView {
    /// Racine : une entrée par groupe.
    #[default]
    Root,
    /// Contenu d'un groupe.
    Group(PaletteGroup),
    /// Saisie guidée : le champ de recherche devient un champ de valeur.
    Prompt(PromptKind),
}

impl PaletteView {
    /// Fil d'Ariane affiché en tête de la barre de recherche.
    #[must_use]
    pub const fn breadcrumb(self) -> Option<&'static str> {
        match self {
            Self::Root => None,
            Self::Group(group) => Some(group.title()),
            Self::Prompt(prompt) => Some(prompt.breadcrumb()),
        }
    }

    /// Indique si le champ de saisie sert à autre chose qu'à rechercher.
    #[must_use]
    pub const fn is_prompt(self) -> bool {
        matches!(self, Self::Prompt(_))
    }
}

/// Type d'action déclenchable lors de la validation d'un item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteAction {
    /// Équipe ou déséquipe un accessoire modulaire.
    ToggleAccessory {
        /// Emplacement concerné.
        category: AccessoryCategory,
        /// Identifiant de l'accessoire.
        id: String,
    },
    /// Caresse le Gremlin pour augmenter son bonheur.
    PetGremlin,
    /// Réanime un Gremlin décédé.
    RevivePet,
    /// Consomme un objet de l'inventaire.
    ///
    /// Remplace les anciennes actions illimitées « nourrir » et « soigner » :
    /// les laisser à côté de l'inventaire l'aurait rendu purement décoratif.
    UseConsumable(ConsumableKind),
    /// Active ou désactive le minuteur de concentration.
    TogglePomodoro,
    /// Démarre un bloc de concentration.
    StartPomodoro,
    /// Suspend le minuteur.
    PausePomodoro,
    /// Reprend le minuteur suspendu.
    ResumePomodoro,
    /// Arrête le cycle de concentration.
    StopPomodoro,
    /// Passe la pause en cours et prépare le bloc suivant.
    SkipPomodoroBreak,
    /// Active ou désactive la chute douce au lâcher.
    ToggleDesktopMotion,
    /// Active ou désactive l'ancrage aux bords de la zone de travail.
    ToggleDesktopMagnetism,
    /// Bascule le mode sommeil du Gremlin.
    ToggleSleep,
    /// Bascule le mode click-through (la souris traverse la fenêtre).
    ToggleClickThrough,
    /// Bascule le lancement automatique au démarrage de l'OS.
    ToggleAutostart,
    /// Bascule la détection passive des rapports de tests et builds.
    ToggleToolingWatcher,
    /// Bascule l'estimation des sessions de focus.
    ToggleFocusTracking,
    /// Bascule les rappels de pause liés au focus.
    ToggleBreakReminders,
    /// Applique l'échelle de fenêtre suivante du cycle.
    ///
    /// La valeur est calculée à la construction de l'item depuis la
    /// configuration : elle ne se déduit plus du texte du badge affiché.
    CycleScaleFactor {
        /// Échelle qui sera appliquée à la validation.
        next: u32,
    },
    /// Déclenche le rechargement à chaud de tous les assets.
    ReloadAssets,
    /// Ouvre le dossier utilisateur des skins et mods dans l'OS.
    OpenModsFolder,
    /// Ouvre le dossier utilisateur des données et sauvegardes.
    OpenDataFolder,
    /// Force une sauvegarde immédiate de l'état.
    SaveNow,
    /// Applique la taille de texte suivante du cycle.
    CycleTextSize,
    /// Applique le thème suivant du cycle.
    CycleTheme,
    /// Bascule la réduction des animations.
    ToggleReducedMotion,
    /// Bascule la fermeture du panneau à la perte de focus.
    ToggleCloseOnFocusLoss,
    /// Descend dans un groupe de la racine.
    ///
    /// Traitée intégralement par la palette : la navigation est un état
    /// d'interface, elle n'a pas à remonter jusqu'à l'orchestrateur.
    EnterGroup(PaletteGroup),
    /// Ouvre le mode saisie du chemin d'un dépôt à ajouter.
    ///
    /// Comme [`Self::EnterGroup`], c'est de la navigation : la palette la traite
    /// elle-même.
    PromptAddTrackedRepo,
    /// Ouvre le sélecteur de dossier du système.
    BrowseForTrackedRepo,
    /// Confie un dépôt à la surveillance.
    AddTrackedRepo(PathBuf),
    /// Retire un dépôt de la surveillance et de la configuration.
    RemoveTrackedRepo(PathBuf),
    /// Ouvre le dossier d'un dépôt suivi dans le gestionnaire de fichiers.
    OpenRepoFolder(PathBuf),
    /// Action sans effet direct.
    None,
}

/// Pictogramme d'une action secondaire de ligne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowActionIcon {
    /// Corbeille : retire l'élément de la liste.
    Trash,
}

/// Action secondaire d'une ligne, déclenchée par son propre bouton.
///
/// Volontairement générique : le moteur de rendu dessine un pictogramme et une
/// zone cliquable sans jamais apprendre ce qu'est un dépôt Git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowAction {
    /// Pictogramme dessiné dans la marge droite de la ligne.
    pub icon: RowActionIcon,
    /// Libellé annoncé par les lecteurs d'écran.
    ///
    /// Sans lui, le bouton n'existerait pas à l'oreille : une zone cliquable
    /// muette est une fonctionnalité réservée à ceux qui la voient.
    pub label: String,
    /// Action déclenchée par le bouton.
    pub action: PaletteAction,
}

/// Un item sélectionnable dans la palette de commande.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItem {
    /// Identifiant stable de l'item.
    pub id: String,
    /// Titre affiché dans la liste.
    pub title: String,
    /// Sous-titre descriptif.
    pub subtitle: String,
    /// Section de regroupement.
    pub section: PaletteSection,
    /// Emplacement d'accessoire concerné, le cas échéant.
    pub category: Option<AccessoryCategory>,
    /// Badge de statut affiché à droite.
    pub badge: Option<String>,
    /// Indique si l'item représente un état actif.
    pub is_equipped: bool,
    /// Action déclenchée à la validation.
    pub action: PaletteAction,
    /// Métadonnées libres exploitées par le panneau d'inspection.
    pub metadata: HashMap<String, String>,
    /// Action secondaire portée par la ligne, avec son propre bouton.
    pub row_action: Option<RowAction>,
}

/// Résultat d'une action déclenchée par l'utilisateur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteExecutionResult {
    /// Équipe un accessoire.
    EquipAccessory {
        /// Emplacement concerné.
        category: AccessoryCategory,
        /// Identifiant de l'accessoire.
        id: String,
    },
    /// Retire l'accessoire d'un emplacement.
    UnequipAccessory {
        /// Emplacement concerné.
        category: AccessoryCategory,
    },
    /// Caresse le familier.
    PetGremlin,
    /// Réanime le familier.
    RevivePet,
    /// Consomme un objet de l'inventaire.
    UseConsumable(ConsumableKind),
    /// Active ou désactive le minuteur de concentration.
    TogglePomodoro,
    /// Démarre un bloc de concentration.
    StartPomodoro,
    /// Suspend le minuteur.
    PausePomodoro,
    /// Reprend le minuteur suspendu.
    ResumePomodoro,
    /// Arrête le cycle de concentration.
    StopPomodoro,
    /// Passe la pause en cours.
    SkipPomodoroBreak,
    /// Active ou désactive la chute douce.
    ToggleDesktopMotion,
    /// Active ou désactive l'ancrage aux bords.
    ToggleDesktopMagnetism,
    /// Bascule le sommeil.
    ToggleSleep,
    /// Bascule le mode click-through.
    ToggleClickThrough,
    /// Bascule l'autostart.
    ToggleAutostart,
    /// Bascule la surveillance des rapports d'outillage.
    ToggleToolingWatcher,
    /// Bascule l'estimation des sessions de focus.
    ToggleFocusTracking,
    /// Bascule les rappels de pause.
    ToggleBreakReminders,
    /// Applique une échelle de fenêtre.
    SetScaleFactor(u32),
    /// Recharge les assets.
    ReloadAssets,
    /// Ouvre le dossier des mods.
    OpenModsFolder,
    /// Ouvre le dossier des données.
    OpenDataFolder,
    /// Sauvegarde immédiatement.
    SaveNow,
    /// Applique la taille de texte suivante.
    CycleTextSize,
    /// Applique le thème suivant.
    CycleTheme,
    /// Bascule la réduction des animations.
    ToggleReducedMotion,
    /// Bascule la fermeture à la perte de focus.
    ToggleCloseOnFocusLoss,
    /// Confie un dépôt Git à la surveillance.
    AddTrackedRepo(PathBuf),
    /// Retire un dépôt de la surveillance et de la configuration.
    RemoveTrackedRepo(PathBuf),
    /// Ouvre le dossier d'un dépôt suivi.
    OpenRepoFolder(PathBuf),
    /// Ouvre le sélecteur de dossier du système.
    BrowseForTrackedRepo,
    /// Aucune action.
    None,
}

/// État de surveillance d'un dépôt déclaré.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoTrackingStatus {
    /// Surveillé activement : commits, branches et rapports y sont détectés.
    Active,
    /// Déclaré mais non surveillé : chemin introuvable, dépôt supprimé, ou
    /// surveillance indisponible. Le dépôt reste listé — et donc retirable.
    Unavailable,
}

impl RepoTrackingStatus {
    /// Pastille affichée à droite de la ligne.
    #[must_use]
    pub const fn badge(self) -> &'static str {
        match self {
            Self::Active => "SUIVI",
            Self::Unavailable => "INDISPONIBLE",
        }
    }
}

/// Informations sommaires d'un dépôt Git suivi, pour l'affichage Raycast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoDisplayInfo {
    /// Chemin déclaré du dépôt.
    ///
    /// Identifiant de la ligne, et non son nom : deux dépôts nommés `api` sous
    /// deux racines différentes sont deux entrées distinctes, et le retrait doit
    /// désigner celle que l'utilisateur a visée.
    pub path: PathBuf,
    /// Nom court du dépôt.
    pub name: String,
    /// Branche courante, si elle a pu être lue.
    ///
    /// `None` tant qu'aucun signal n'a renseigné la branche : afficher une
    /// valeur inventée comme « main » produisait de fausses notifications de
    /// bascule de branche au premier commit réel.
    pub branch: Option<String>,
    /// Dernier message de commit connu.
    pub last_commit_msg: Option<String>,
    /// État de la surveillance de ce dépôt.
    pub status: RepoTrackingStatus,
    /// Cause de l'indisponibilité, telle qu'elle sera montrée à l'utilisateur.
    pub issue: Option<String>,
}

impl PaletteItem {
    /// Indique si l'item est une entrée de racine ouvrant un groupe.
    ///
    /// Le rendu s'en sert pour ne pas coiffer ces lignes d'un libellé de
    /// section : elles *sont* les sections, le libellé ne dirait rien de plus
    /// que le titre juste à côté.
    #[must_use]
    pub const fn is_group_entry(&self) -> bool {
        matches!(self.action, PaletteAction::EnterGroup(_))
    }

    /// Indique si l'item représente un réglage à deux états.
    ///
    /// L'arbre d'accessibilité s'en sert pour l'annoncer comme un interrupteur,
    /// avec son état, plutôt que comme une simple ligne de liste dont la
    /// validation resterait mystérieuse à l'oreille.
    #[must_use]
    pub const fn is_toggle(&self) -> bool {
        matches!(
            self.action,
            PaletteAction::ToggleAccessory { .. }
                | PaletteAction::ToggleSleep
                | PaletteAction::ToggleClickThrough
                | PaletteAction::ToggleAutostart
                | PaletteAction::ToggleToolingWatcher
                | PaletteAction::ToggleFocusTracking
                | PaletteAction::ToggleBreakReminders
                | PaletteAction::ToggleReducedMotion
                | PaletteAction::ToggleCloseOnFocusLoss
                | PaletteAction::TogglePomodoro
                | PaletteAction::ToggleDesktopMotion
                | PaletteAction::ToggleDesktopMagnetism
        )
    }

    /// Indique que l'item déclenche une commande immédiate.
    ///
    /// Le lecteur d'écran l'annonce alors comme un **bouton** : « Café, bouton »
    /// dit ce que la validation va faire, là où « élément de liste » laisse
    /// deviner.
    #[must_use]
    pub const fn is_command_button(&self) -> bool {
        matches!(
            self.action,
            PaletteAction::UseConsumable(_)
                | PaletteAction::StartPomodoro
                | PaletteAction::PausePomodoro
                | PaletteAction::ResumePomodoro
                | PaletteAction::StopPomodoro
                | PaletteAction::SkipPomodoroBreak
                | PaletteAction::PetGremlin
                | PaletteAction::RevivePet
        )
    }

    /// Indique que la ligne est purement informative.
    ///
    /// Une ligne sans action ne doit pas annoncer qu'elle est activable : c'est
    /// le cas du résumé de série et d'une récompense encore verrouillée, qui
    /// restent lisibles mais ne déclenchent rien.
    #[must_use]
    pub const fn is_informational(&self) -> bool {
        matches!(self.action, PaletteAction::None)
    }
}

impl RepoDisplayInfo {
    /// Construit l'entrée d'un dépôt déclaré, avant tout signal du watcher.
    #[must_use]
    pub fn declared(path: PathBuf, status: RepoTrackingStatus, issue: Option<String>) -> Self {
        let name = repo_name_from_path(&path);
        Self {
            path,
            name,
            branch: None,
            last_commit_msg: None,
            status,
            issue,
        }
    }

    /// Libellé de branche affichable, y compris lorsqu'elle est inconnue.
    #[must_use]
    pub fn branch_label(&self) -> &str {
        self.branch.as_deref().unwrap_or("inconnue")
    }
}

/// Nom court d'un dépôt, déduit du dernier composant de son chemin.
///
/// Une racine de volume (`C:\`, `/`) n'a pas de dernier composant : le chemin
/// complet est alors conservé, ce qui vaut mieux qu'une ligne sans nom.
#[must_use]
pub fn repo_name_from_path(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Contexte de construction des items de la palette.
///
/// Regrouper ces références évite une signature à sept paramètres positionnels
/// et permet d'ajouter une donnée d'affichage sans casser tous les appelants.
#[derive(Debug, Clone, Copy)]
pub struct PaletteContext<'a> {
    /// Catalogue des accessoires disponibles.
    pub catalog: &'a AccessoryCatalog,
    /// Équipement actuellement porté.
    pub wardrobe: &'a WardrobeEquipment,
    /// État du familier.
    pub pet_state: &'a PetState,
    /// Configuration applicative.
    pub config: &'a AppConfig,
    /// Indique si le lancement automatique est réellement actif au niveau OS.
    pub autostart_active: bool,
    /// Dépôts Git déclarés par l'utilisateur.
    pub repos: &'a [RepoDisplayInfo],
    /// Répertoire de lancement, s'il se trouve être un dépôt Git.
    ///
    /// Résolu une seule fois au démarrage : il ne change pas pendant la vie du
    /// processus, et le recalculer à chaque reconstruction de la liste ferait
    /// toucher au disque pour rien.
    pub current_dir_repo: Option<&'a Path>,
    /// Indique si le sélecteur de dossier du système est utilisable.
    pub folder_picker_available: bool,
    /// Dernière erreur de sauvegarde à signaler à l'utilisateur.
    pub last_save_error: Option<&'a str>,
    /// Dernier incident du watcher ou du moniteur d'activité.
    pub last_observation_error: Option<&'a str>,
    /// État d'outillage demandé, tant que le worker ne l'a pas confirmé.
    pub pending_tooling_enabled: Option<bool>,
    /// Jour civil courant, absent si le calendrier local est indisponible.
    ///
    /// Sans lui la série n'est pas calculable : l'interface le dit au lieu
    /// d'afficher un zéro qui passerait pour une série rompue.
    pub today: Option<CivilDate>,
    /// Indique que le placement natif est exploitable sur cette plateforme.
    pub desktop_placement_available: bool,
    /// Raison de l'indisponibilité du placement natif, le cas échéant.
    pub desktop_unavailable_reason: Option<&'a str>,
}

/// Formate un temps restant en `MM:SS`, arrondi à la seconde.
///
/// L'arrondi est volontaire : le panneau n'affiche pas les dixièmes, et
/// rafraîchir plus finement ferait vivre l'application pour rien.
fn format_remaining(remaining: std::time::Duration) -> String {
    let seconds = remaining.as_secs();
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// État de la palette de commande et gestionnaire de filtrage.
#[derive(Debug, Clone)]
pub struct CommandPalette {
    query: String,
    /// Position du curseur de saisie, en octets dans `query`.
    ///
    /// Toujours sur une frontière de caractère : la saisie n'était auparavant
    /// possible qu'en fin de chaîne, `pop_char` retirant systématiquement le
    /// dernier caractère quelle que soit l'intention de l'utilisateur.
    caret: usize,
    view: PaletteView,
    selected_index: usize,
    /// Feuilles : les items réellement actionnables.
    all_items: Vec<PaletteItem>,
    /// Entrées de racine, une par groupe non vide.
    root_items: Vec<PaletteItem>,
    /// Ligne de confirmation du mode saisie, reconstruite à chaque frappe.
    prompt_item: Option<PaletteItem>,
    /// Lignes retenues par le filtre courant.
    ///
    /// Stocker des références plutôt que des copies évite de cloner la totalité
    /// des items — `HashMap` de métadonnées comprises — à chaque frappe.
    filtered: Vec<FilteredRef>,
}

/// Référence vers une ligne retenue par le filtre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilteredRef {
    /// Entrée de racine, indice dans `root_items`.
    Root(usize),
    /// Feuille, indice dans `all_items`.
    Leaf(usize),
    /// Unique ligne du mode saisie.
    Prompt,
}

impl CommandPalette {
    /// Crée une nouvelle palette de commande initialisée avec tous les éléments disponibles.
    #[must_use]
    pub fn new(context: &PaletteContext<'_>) -> Self {
        let mut palette = Self {
            query: String::new(),
            caret: 0,
            view: PaletteView::Root,
            selected_index: 0,
            all_items: Vec::new(),
            root_items: Vec::new(),
            prompt_item: None,
            filtered: Vec::new(),
        };

        palette.rebuild_items(context);
        palette
    }

    /// Reconstruit la liste complète des items en fonction de l'état actuel de l'application.
    #[allow(clippy::too_many_lines)]
    pub fn rebuild_items(&mut self, context: &PaletteContext<'_>) {
        let PaletteContext {
            catalog,
            wardrobe,
            pet_state,
            config,
            autostart_active,
            repos,
            current_dir_repo,
            folder_picker_available,
            last_save_error,
            last_observation_error,
            pending_tooling_enabled,
            today,
            desktop_placement_available,
            desktop_unavailable_reason,
        } = *context;

        let progression = pet_state.progression();
        let stats = pet_state.stats();
        let core_config = pet_state.config();
        let core_actions = core_config.actions;
        let is_critical = stats.is_critical(&core_config.mood);
        let mut items = Vec::new();

        // 1. Profil & métriques du Gremlin
        let next_level_xp =
            gremlin_core::PetProgression::total_xp_for_level(progression.level().saturating_add(1));

        let mut meta_profile = HashMap::new();
        meta_profile.insert("name".into(), pet_state.name().to_owned());
        meta_profile.insert("stage".into(), progression.stage().display_name().into());
        meta_profile.insert("level".into(), format!("Niveau {}", progression.level()));
        meta_profile.insert(
            "xp".into(),
            format!("{} / {} XP", progression.total_xp(), next_level_xp),
        );
        meta_profile.insert(
            "commits".into(),
            format!("{} commits", progression.total_commits()),
        );
        meta_profile.insert("mood".into(), pet_state.mood().display_name().into());
        meta_profile.insert("satiety".into(), format!("{:.0}%", stats.satiety()));
        meta_profile.insert("energy".into(), format!("{:.0}%", stats.energy()));
        meta_profile.insert("happiness".into(), format!("{:.0}%", stats.happiness()));
        meta_profile.insert(
            "tests".into(),
            format!(
                "{} réussis, {} échoués",
                progression.total_tests_passed(),
                progression.total_tests_failed()
            ),
        );
        meta_profile.insert(
            "focus".into(),
            format!("{} min estimées", progression.total_focus_secs() / 60),
        );
        if is_critical {
            meta_profile.insert(
                "description".into(),
                "Au moins une jauge vitale est au plus bas : intervenez sans tarder.".into(),
            );
        }

        items.push(PaletteItem {
            id: "profile_pet".into(),
            title: format!(
                "{} (Niv. {} - {})",
                pet_state.name(),
                progression.level(),
                progression.stage().display_name()
            ),
            subtitle: format!(
                "Humeur : {} • Bonheur : {:.0}%",
                pet_state.mood().display_name(),
                stats.happiness()
            ),
            section: PaletteSection::PetProfile,
            category: None,
            // Une jauge au plus bas prime sur l'humeur dans le badge : c'est
            // l'information sur laquelle l'utilisateur doit agir.
            badge: Some(if is_critical {
                String::from("CRITIQUE")
            } else {
                pet_state.mood().display_name().to_uppercase()
            }),
            is_equipped: true,
            action: PaletteAction::PetGremlin,
            metadata: meta_profile,
            row_action: None,
        });

        // 2. Soins et actions sur le familier.
        //
        // Nourrir et soigner passent désormais par l'inventaire : garder ici des
        // équivalents illimités aurait rendu les consommables décoratifs.
        // Caresser et réanimer restent des actions directes — elles ne
        // consomment rien.
        let pet_amount = core_actions.default_pet_happiness;
        let mut meta_pet = HashMap::new();
        meta_pet.insert("name".into(), "Caresser Gremlin".into());
        meta_pet.insert(
            "description".into(),
            format!(
                "Caresse le familier pour lui remonter le moral (+{pet_amount:.0} bonheur).                  Équivaut à un clic bref sur le familier lorsque le mode interactif est actif."
            ),
        );
        items.push(PaletteItem {
            id: "care_pet".into(),
            title: "Caresser le familier".into(),
            subtitle: format!("Bonheur actuel : {:.0}%", stats.happiness()),
            section: PaletteSection::PetCare,
            category: None,
            badge: Some(format!("+{pet_amount:.0} BONHEUR")),
            is_equipped: false,
            action: PaletteAction::PetGremlin,
            metadata: meta_pet,
            row_action: None,
        });

        let is_sleeping = pet_state.is_sleeping();
        let mut meta_sleep = HashMap::new();
        meta_sleep.insert("name".into(), "Mode Sommeil".into());
        meta_sleep.insert(
            "description".into(),
            "Met Gremlin en pause / sommeil pour réduire la décroissance.".into(),
        );
        items.push(PaletteItem {
            id: "care_sleep".into(),
            title: if is_sleeping {
                "Réveiller Gremlin".into()
            } else {
                "Endormir Gremlin (Mode Pause)".into()
            },
            subtitle: if is_sleeping {
                "Gremlin dort paisiblement".into()
            } else {
                "Prêt pour une sieste réparatrice".into()
            },
            section: PaletteSection::PetCare,
            category: None,
            badge: Some(if is_sleeping { "DORT" } else { "ÉVEILLÉ" }.into()),
            is_equipped: is_sleeping,
            action: PaletteAction::ToggleSleep,
            metadata: meta_sleep,
            row_action: None,
        });

        if !pet_state.is_alive() {
            let mut meta_revive = HashMap::new();
            meta_revive.insert("name".into(), "Réanimer Gremlin".into());
            meta_revive.insert(
                "description".into(),
                "Ressuscite votre Gremlin et réinitialise ses constantes vitales.".into(),
            );
            items.push(PaletteItem {
                id: "care_revive".into(),
                title: "Réanimer Gremlin (Renaissance)".into(),
                subtitle: "Redonne vie à votre fidèle compagnon".into(),
                section: PaletteSection::PetCare,
                category: None,
                badge: Some("RÉSURRECTION".into()),
                is_equipped: false,
                action: PaletteAction::RevivePet,
                metadata: meta_revive,
                row_action: None,
            });
        }

        // 3. Catégories d'accessoires (garde-robe)
        let categories = [
            (AccessoryCategory::Hat, PaletteSection::Hats),
            (AccessoryCategory::Glasses, PaletteSection::Glasses),
            (AccessoryCategory::Outfit, PaletteSection::Outfits),
            (AccessoryCategory::Held, PaletteSection::Held),
            (AccessoryCategory::Aura, PaletteSection::Auras),
        ];

        for (cat, section) in categories {
            for item in catalog.items_by_category(cat) {
                let is_equipped = wardrobe.is_equipped_in(cat, item.id());
                let badge = if is_equipped {
                    Some(String::from("ÉQUIPÉ"))
                } else if matches!(item.source, AccessorySource::BuiltIn) {
                    Some(String::from("INTÉGRÉ"))
                } else {
                    Some(String::from("MOD"))
                };

                let mut meta = HashMap::new();
                meta.insert("name".into(), item.manifest.name.clone());
                meta.insert("author".into(), item.manifest.author.clone());
                meta.insert("version".into(), item.manifest.version.clone());
                meta.insert("description".into(), item.manifest.description.clone());
                meta.insert("category".into(), cat.display_name().into());

                items.push(PaletteItem {
                    id: item.id().to_string(),
                    title: item.manifest.name.clone(),
                    subtitle: format!("{}: {}", cat.display_name(), item.manifest.description),
                    section,
                    category: Some(cat),
                    badge,
                    is_equipped,
                    action: PaletteAction::ToggleAccessory {
                        category: cat,
                        id: item.id().to_string(),
                    },
                    metadata: meta,
                    row_action: None,
                });
            }
        }

        // 4. Dépôts Git suivis — actions d'ajout, puis un item par dépôt déclaré.
        //
        // Ces actions sont permanentes : le groupe « Dépôts surveillés » reste
        // donc visible à la racine même sans aucun dépôt, ce qui donne à l'état
        // vide un endroit où s'expliquer.
        let mut meta_add = HashMap::new();
        meta_add.insert("name".into(), "Ajouter un dépôt Git".into());
        meta_add.insert(
            "description".into(),
            "Indiquez le chemin absolu d'un dépôt : Gremlin surveillera ses commits, ses branches et ses rapports de tests."
                .into(),
        );
        items.push(PaletteItem {
            id: "repo_add_by_path".into(),
            title: "Ajouter un dépôt Git…".into(),
            subtitle: if repos.is_empty() {
                "Aucun dépôt surveillé — commencez ici".into()
            } else {
                "Saisir le chemin d'un dépôt à surveiller".into()
            },
            section: PaletteSection::GitWatcher,
            category: None,
            badge: Some("+".into()),
            is_equipped: false,
            action: PaletteAction::PromptAddTrackedRepo,
            metadata: meta_add,
            row_action: None,
        });

        if folder_picker_available {
            let mut meta_browse = HashMap::new();
            meta_browse.insert("name".into(), "Parcourir un dossier".into());
            meta_browse.insert(
                "description".into(),
                "Ouvre le sélecteur de dossier du système pour désigner un dépôt.".into(),
            );
            items.push(PaletteItem {
                id: "repo_browse".into(),
                title: "Parcourir un dossier…".into(),
                subtitle: "Sélecteur de dossier du système".into(),
                section: PaletteSection::GitWatcher,
                category: None,
                badge: Some("DOSSIER".into()),
                is_equipped: false,
                action: PaletteAction::BrowseForTrackedRepo,
                metadata: meta_browse,
                row_action: None,
            });
        }

        // Le répertoire de lancement n'est proposé que s'il est un dépôt Git et
        // qu'il n'est pas déjà suivi : sinon la ligne ne ferait rien.
        if let Some(current) = current_dir_repo {
            if !repos.iter().any(|repo| repo.path == current) {
                let name = repo_name_from_path(current);
                let mut meta_current = HashMap::new();
                meta_current.insert("name".into(), name.clone());
                meta_current.insert("path".into(), current.display().to_string());
                meta_current.insert(
                    "description".into(),
                    "Répertoire depuis lequel Gremlin a été lancé.".into(),
                );
                items.push(PaletteItem {
                    id: "repo_add_current_dir".into(),
                    title: format!("Ajouter le dossier courant : {name}"),
                    subtitle: current.display().to_string(),
                    section: PaletteSection::GitWatcher,
                    category: None,
                    badge: Some("COURANT".into()),
                    is_equipped: false,
                    action: PaletteAction::AddTrackedRepo(current.to_path_buf()),
                    metadata: meta_current,
                    row_action: None,
                });
            }
        }

        for repo in repos {
            let mut meta_repo = HashMap::new();
            meta_repo.insert("name".into(), repo.name.clone());
            meta_repo.insert("path".into(), repo.path.display().to_string());
            meta_repo.insert("branch".into(), repo.branch_label().to_owned());
            if let Some(ref msg) = repo.last_commit_msg {
                meta_repo.insert("last_commit".into(), msg.clone());
            }
            meta_repo.insert(
                "description".into(),
                repo.issue.clone().unwrap_or_else(|| {
                    "Commits, bascules de branche et rapports de tests y sont détectés.".to_owned()
                }),
            );

            let subtitle = repo.issue.as_ref().map_or_else(
                || {
                    format!(
                        "Branche : {} • Dernier commit : {}",
                        repo.branch_label(),
                        repo.last_commit_msg.as_deref().unwrap_or("aucun")
                    )
                },
                // Pas de pictogramme d'alerte ici : la pastille « INDISPONIBLE »
                // porte déjà l'état, le sous-titre n'a plus qu'à en donner la
                // cause. Le « ⚠ » est réservé aux incidents dont la pastille ne
                // dit rien — celle de l'outillage affiche ON ou OFF, jamais
                // l'incident lui-même.
                Clone::clone,
            );

            items.push(PaletteItem {
                // Le chemin fait l'identité : deux dépôts homonymes doivent
                // rester deux lignes distinctes et retirables séparément.
                id: format!("repo_{}", repo.path.display()),
                title: format!("Dépôt : {}", repo.name),
                subtitle,
                section: PaletteSection::GitWatcher,
                category: None,
                badge: Some(match repo.status {
                    RepoTrackingStatus::Active => repo.branch_label().to_owned(),
                    RepoTrackingStatus::Unavailable => repo.status.badge().to_owned(),
                }),
                is_equipped: false,
                action: PaletteAction::OpenRepoFolder(repo.path.clone()),
                metadata: meta_repo,
                row_action: Some(RowAction {
                    icon: RowActionIcon::Trash,
                    label: format!("Retirer {} de la surveillance", repo.name),
                    action: PaletteAction::RemoveTrackedRepo(repo.path.clone()),
                }),
            });
        }

        // 5. Productivité : série de commits, inventaire et concentration.
        let productivity = pet_state.productivity();
        let streak = productivity.streak();
        let inventory = productivity.inventory();
        let timer = productivity.pomodoro();
        let streak_config = core_config.streak;

        // 5a. Série de commits.
        let mut meta_streak = HashMap::new();
        meta_streak.insert("name".into(), "Série de commits".into());
        // `if let` plutôt que `Option::map_or_else` : les deux branches
        // renseignent la même table de métadonnées, et deux fermetures ne
        // peuvent pas l'emprunter en écriture simultanément.
        let streak_subtitle = if let Some(today) = today {
            let snapshot = streak.snapshot(today);
            meta_streak.insert(
                "current".into(),
                format!("{} jour(s) d'affilée", snapshot.current_days),
            );
            meta_streak.insert(
                "longest".into(),
                format!("record : {} jour(s)", snapshot.longest_days),
            );
            meta_streak.insert(
                "total".into(),
                format!(
                    "{} jour(s) de commits au total",
                    snapshot.total_productive_days
                ),
            );
            if let Some(last) = snapshot.last_active_day {
                meta_streak.insert("last".into(), format!("dernier commit le {last}"));
            }
            let next = streak.next_milestone(today, streak_config).map_or_else(
                || String::from("toutes les récompenses sont acquises"),
                |(reward, remaining)| format!("{remaining} jour(s) avant « {} »", reward.label()),
            );
            meta_streak.insert("next".into(), next);
            format!(
                "{} jour(s) d'affilée • record {} jour(s)",
                snapshot.current_days, snapshot.longest_days
            )
        } else {
            // Sans date locale, afficher « 0 » ferait passer une série intacte
            // pour une série rompue.
            meta_streak.insert(
                "description".into(),
                "La date locale est indisponible : la série ne peut pas être calculée. \
                 Les jours déjà enregistrés sont conservés."
                    .into(),
            );
            String::from("Date locale indisponible")
        };
        items.push(PaletteItem {
            id: "productivity_streak".into(),
            title: "Série de commits".into(),
            subtitle: streak_subtitle,
            section: PaletteSection::Streak,
            category: None,
            badge: Some(today.map_or_else(
                || String::from("INDISPONIBLE"),
                |today| format!("{} J", streak.current_streak(today)),
            )),
            is_equipped: today.is_some_and(|today| streak.current_streak(today) > 0),
            action: PaletteAction::None,
            metadata: meta_streak,
            row_action: None,
        });

        // 5b. Récompenses de série, acquises comme verrouillées.
        //
        // Une récompense verrouillée reste lisible et annonce son critère : la
        // griser sans texte la rendrait muette au lecteur d'écran.
        for (reward, required) in StreakReward::ALL
            .into_iter()
            .zip(streak_config.milestone_days)
        {
            let unlocked = streak.is_unlocked(reward);
            let mut meta = HashMap::new();
            meta.insert("name".into(), reward.label().to_owned());
            meta.insert("accessory".into(), reward.accessory_id().to_owned());
            meta.insert(
                "description".into(),
                if unlocked {
                    format!(
                        "Débloqué à {required} jours consécutifs. Disponible dans la garde-robe."
                    )
                } else {
                    format!(
                        "Se débloque après {required} jours de commits consécutifs. \
                         Non équipable avant cela."
                    )
                },
            );
            items.push(PaletteItem {
                id: format!("streak_reward_{}", reward.accessory_id()),
                title: reward.label().to_owned(),
                subtitle: if unlocked {
                    format!("Acquis • {required} jours consécutifs")
                } else {
                    format!("Verrouillé • {required} jours consécutifs requis")
                },
                section: PaletteSection::Streak,
                category: None,
                badge: Some(if unlocked { "ACQUIS" } else { "VERROUILLÉ" }.into()),
                is_equipped: unlocked,
                action: PaletteAction::None,
                metadata: meta,
                row_action: None,
            });
        }

        // 5c. Inventaire : un item par consommable, utilisable ou refusé.
        for (index, kind) in ConsumableKind::ALL.into_iter().enumerate() {
            let quantity = inventory.quantity(kind);
            let effect = kind.potential_effect(stats, &core_config.inventory);
            let refusal = if !pet_state.is_alive() {
                Some("Gremlin est décédé")
            } else if pet_state.is_sleeping() {
                Some("Gremlin dort")
            } else if quantity == 0 {
                Some("stock vide")
            } else if !effect.is_meaningful() {
                Some("jauge déjà pleine")
            } else {
                None
            };

            let mut meta = HashMap::new();
            meta.insert("name".into(), kind.title().to_owned());
            meta.insert("stock".into(), format!("{quantity} en stock"));
            meta.insert(
                "effect".into(),
                format!(
                    "+{:.0} énergie, +{:.0} satiété, +{:.0} bonheur",
                    effect.energy, effect.satiety, effect.happiness
                ),
            );
            meta.insert(
                "description".into(),
                refusal.map_or_else(
                    || {
                        format!(
                            "Raccourci {} • ou glissez l'objet sur le familier de l'aperçu.",
                            index + 1
                        )
                    },
                    |reason| format!("Inutilisable pour l'instant : {reason}."),
                ),
            );

            items.push(PaletteItem {
                id: format!("inventory_{}", kind.id()),
                title: kind.title().to_owned(),
                subtitle: refusal.map_or_else(
                    || {
                        format!(
                            "{quantity} en stock • +{:.0} énergie, +{:.0} satiété, +{:.0} bonheur",
                            effect.energy, effect.satiety, effect.happiness
                        )
                    },
                    |reason| format!("{quantity} en stock • {reason}"),
                ),
                section: PaletteSection::Inventory,
                category: None,
                // « STOCK n » plutôt que « ×n » : la police dessinée à la main
                // ne couvre pas le signe multiplié, qui sortirait en tofu.
                badge: Some(format!("STOCK {quantity}")),
                is_equipped: refusal.is_none(),
                action: PaletteAction::UseConsumable(kind),
                metadata: meta,
                row_action: None,
            });
        }

        // 5d. Minuteur de concentration.
        let pomodoro_enabled = config.pomodoro_enabled;
        let mut meta_timer = HashMap::new();
        meta_timer.insert("name".into(), "Minuteur de concentration".into());
        meta_timer.insert(
            "description".into(),
            format!(
                "Cycle {} min de travail / {} min de pause, pause longue de {} min tous les {} blocs. \
                 Aucune récompense : le minuteur favorise la santé, pas l'accumulation.",
                core_config.pomodoro.work_secs / 60,
                core_config.pomodoro.short_break_secs / 60,
                core_config.pomodoro.long_break_secs / 60,
                core_config.pomodoro.blocks_before_long_break,
            ),
        );
        items.push(PaletteItem {
            id: "focus_timer_enabled".into(),
            title: "Minuteur de concentration".into(),
            subtitle: if pomodoro_enabled {
                "Activé • le démarrage de chaque phase reste volontaire".into()
            } else {
                "Désactivé • aucun temps mesuré ni rappel affiché".into()
            },
            section: PaletteSection::FocusTimer,
            category: None,
            badge: Some(
                if pomodoro_enabled {
                    "ACTIVÉ"
                } else {
                    "DÉSACTIVÉ"
                }
                .into(),
            ),
            is_equipped: pomodoro_enabled,
            action: PaletteAction::TogglePomodoro,
            metadata: meta_timer,
            row_action: None,
        });

        if pomodoro_enabled {
            // Seules les transitions réellement possibles sont proposées : une
            // commande affichée puis refusée serait un faux bouton.
            let timer_state = timer.state();
            let mut meta_status = HashMap::new();
            meta_status.insert("name".into(), "État du minuteur".into());
            let (status_title, status_badge, status_subtitle) = match timer_state {
                PomodoroState::Idle => (
                    "Démarrer un bloc de concentration",
                    "ARRÊTÉ".to_owned(),
                    format!("{} min de travail", core_config.pomodoro.work_secs / 60),
                ),
                PomodoroState::Running(session) => (
                    "Mettre en pause",
                    format_remaining(session.remaining()),
                    format!(
                        "{} en cours • {} bloc(s) accompli(s)",
                        session.phase().label(),
                        session.completed_work_blocks()
                    ),
                ),
                PomodoroState::Paused(session, reason) => (
                    match session.phase() {
                        PomodoroPhase::Work => "Reprendre la concentration",
                        PomodoroPhase::ShortBreak | PomodoroPhase::LongBreak => {
                            "Commencer la pause"
                        }
                    },
                    format_remaining(session.remaining()),
                    format!("{} • {}", session.phase().label(), reason.label()),
                ),
            };
            meta_status.insert("description".into(), status_subtitle.clone());
            items.push(PaletteItem {
                id: "focus_timer_status".into(),
                title: status_title.to_owned(),
                subtitle: status_subtitle,
                section: PaletteSection::FocusTimer,
                category: None,
                badge: Some(status_badge),
                is_equipped: timer.is_running(),
                action: match timer_state {
                    PomodoroState::Idle => PaletteAction::StartPomodoro,
                    PomodoroState::Running(_) => PaletteAction::PausePomodoro,
                    PomodoroState::Paused(_, _) => PaletteAction::ResumePomodoro,
                },
                metadata: meta_status,
                row_action: None,
            });

            if let PomodoroState::Running(session) | PomodoroState::Paused(session, _) = timer_state
            {
                if session.phase().is_break() {
                    let mut meta_skip = HashMap::new();
                    meta_skip.insert("name".into(), "Passer la pause".into());
                    meta_skip.insert(
                        "description".into(),
                        "Prépare le bloc de travail suivant. Un bloc de travail, lui, ne se \
                         saute pas : il serait comptabilisé sans avoir été accompli."
                            .into(),
                    );
                    items.push(PaletteItem {
                        id: "focus_timer_skip".into(),
                        title: "Passer la pause".into(),
                        subtitle: "Enchaîne sur le bloc de travail suivant".into(),
                        section: PaletteSection::FocusTimer,
                        category: None,
                        badge: Some("PASSER".into()),
                        is_equipped: false,
                        action: PaletteAction::SkipPomodoroBreak,
                        metadata: meta_skip,
                        row_action: None,
                    });
                }

                let mut meta_stop = HashMap::new();
                meta_stop.insert("name".into(), "Arrêter le cycle".into());
                meta_stop.insert(
                    "description".into(),
                    "Remet le minuteur à l'arrêt. Les blocs déjà accomplis ne sont pas perdus."
                        .into(),
                );
                items.push(PaletteItem {
                    id: "focus_timer_stop".into(),
                    title: "Arrêter le cycle".into(),
                    subtitle: format!("{} bloc(s) accompli(s)", session.completed_work_blocks()),
                    section: PaletteSection::FocusTimer,
                    category: None,
                    badge: Some("ARRÊTER".into()),
                    is_equipped: false,
                    action: PaletteAction::StopPomodoro,
                    metadata: meta_stop,
                    row_action: None,
                });
            }
        }

        // 5e. Placement sur le bureau.
        //
        // Sous un compositeur qui ne publie ni position ni zone de travail, les
        // bascules sont annoncées indisponibles plutôt que présentées comme
        // actives : le réglage n'aurait aucun effet.
        let placement_note = desktop_unavailable_reason
            .map(|reason| format!("Indisponible sur cette plateforme : {reason}"));
        for (id, title, enabled, action, description) in [
            (
                "desktop_motion",
                "Chute douce au lâcher",
                config.desktop_motion_enabled,
                PaletteAction::ToggleDesktopMotion,
                "Le familier retombe doucement vers le bas de la zone de travail après un \
                 déplacement. Le mouvement réduit le place instantanément.",
            ),
            (
                "desktop_magnetism",
                "Ancrage aux bords",
                config.desktop_magnetism_enabled,
                PaletteAction::ToggleDesktopMagnetism,
                "Le familier vient se caler au coin le plus proche lorsqu'il est lâché près \
                 d'un bord, sinon il reste où il tombe.",
            ),
        ] {
            let mut meta = HashMap::new();
            meta.insert("name".into(), title.to_owned());
            meta.insert(
                "description".into(),
                placement_note.as_ref().map_or_else(
                    || description.to_owned(),
                    |note| format!("{description}\n{note}"),
                ),
            );
            items.push(PaletteItem {
                id: id.to_owned(),
                title: title.to_owned(),
                subtitle: placement_note
                    .clone()
                    .unwrap_or_else(|| if enabled { "Activé" } else { "Désactivé" }.to_owned()),
                section: PaletteSection::DesktopBehaviour,
                category: None,
                badge: Some(if !desktop_placement_available {
                    "INDISPONIBLE".to_owned()
                } else if enabled {
                    "ACTIVÉ".to_owned()
                } else {
                    "DÉSACTIVÉ".to_owned()
                }),
                is_equipped: desktop_placement_available && enabled,
                action: if desktop_placement_available {
                    action
                } else {
                    PaletteAction::None
                },
                metadata: meta,
                row_action: None,
            });
        }
        // 6. Actions et préférences système
        let mut meta_autostart = HashMap::new();
        meta_autostart.insert("name".into(), "Lancement au démarrage de l'OS".into());
        meta_autostart.insert(
            "description".into(),
            "Démarre automatiquement Gremlin lors de l'ouverture de session.".into(),
        );
        items.push(PaletteItem {
            id: "setting_autostart".into(),
            title: "Lancement au démarrage (Autostart)".into(),
            subtitle: if autostart_active {
                "Activé (lancement automatique au boot)".into()
            } else {
                "Désactivé (démarrage manuel)".into()
            },
            section: PaletteSection::GeneralSettings,
            category: None,
            badge: Some(if autostart_active { "ON" } else { "OFF" }.into()),
            is_equipped: autostart_active,
            action: PaletteAction::ToggleAutostart,
            metadata: meta_autostart,
            row_action: None,
        });

        let tooling_enabled = config.watcher.tooling_enabled;
        let mut meta_tooling = HashMap::new();
        meta_tooling.insert("name".into(), "Rapports de tests et builds".into());
        meta_tooling.insert(
            "description".into(),
            last_observation_error.map_or_else(
                || {
                    "Surveille passivement les rapports configurés, sans exécuter de commande."
                        .to_owned()
                },
                |error| format!("Dernier incident : {error}"),
            ),
        );
        items.push(PaletteItem {
            id: "setting_tooling_watcher".into(),
            title: "Rapports de tests et builds".into(),
            subtitle: pending_tooling_enabled.map_or_else(
                || {
                    last_observation_error.map_or_else(
                        || "Détection passive des rapports configurés".to_owned(),
                        |error| format!("⚠ {error}"),
                    )
                },
                |enabled| {
                    format!(
                        "{} en cours…",
                        if enabled {
                            "Activation"
                        } else {
                            "Désactivation"
                        }
                    )
                },
            ),
            section: PaletteSection::GeneralSettings,
            category: None,
            badge: Some(if tooling_enabled { "ON" } else { "OFF" }.into()),
            is_equipped: tooling_enabled,
            action: PaletteAction::ToggleToolingWatcher,
            metadata: meta_tooling,
            row_action: None,
        });

        let mut meta_focus_tracking = HashMap::new();
        meta_focus_tracking.insert("name".into(), "Focus estimé".into());
        meta_focus_tracking.insert(
            "description".into(),
            "Estimation locale amorcée par un commit ou un rapport ; aucune fenêtre ni frappe n'est enregistrée."
                .into(),
        );
        items.push(PaletteItem {
            id: "setting_focus_tracking".into(),
            title: "Estimation des sessions de focus".into(),
            subtitle: format!(
                "{} min cumulées • mesure d'activité globale",
                progression.total_focus_secs() / 60
            ),
            section: PaletteSection::GeneralSettings,
            category: None,
            badge: Some(
                if config.focus_tracking_enabled {
                    "ON"
                } else {
                    "OFF"
                }
                .into(),
            ),
            is_equipped: config.focus_tracking_enabled,
            action: PaletteAction::ToggleFocusTracking,
            metadata: meta_focus_tracking,
            row_action: None,
        });

        let mut meta_breaks = HashMap::new();
        meta_breaks.insert("name".into(), "Rappels de pause".into());
        meta_breaks.insert(
            "description".into(),
            "Propose discrètement une pause après une session de focus prolongée.".into(),
        );
        items.push(PaletteItem {
            id: "setting_break_reminders".into(),
            title: "Rappels de pause".into(),
            subtitle: "Notification locale après un focus prolongé".into(),
            section: PaletteSection::GeneralSettings,
            category: None,
            badge: Some(
                if config.break_reminders_enabled {
                    "ON"
                } else {
                    "OFF"
                }
                .into(),
            ),
            is_equipped: config.break_reminders_enabled,
            action: PaletteAction::ToggleBreakReminders,
            metadata: meta_breaks,
            row_action: None,
        });

        // Réglages d'accessibilité, groupés avant les réglages de fenêtre : ce
        // sont eux qui conditionnent la lisibilité de tout le reste.
        let mut meta_text_size = HashMap::new();
        meta_text_size.insert("name".into(), "Taille du texte".into());
        meta_text_size.insert(
            "description".into(),
            "La police est dessinée en bitmap : elle ne suit l'échelle du système que par paliers. Ce réglage permet de choisir le palier."
                .into(),
        );
        items.push(PaletteItem {
            id: "setting_text_size".into(),
            title: format!("Taille du texte : {}", config.ui.text_size.display_name()),
            subtitle: format!(
                "Validez pour passer à « {} »",
                config.ui.text_size.next().display_name()
            ),
            section: PaletteSection::GeneralSettings,
            category: None,
            badge: Some(config.ui.text_size.display_name().to_uppercase()),
            is_equipped: false,
            action: PaletteAction::CycleTextSize,
            metadata: meta_text_size,
            row_action: None,
        });

        let mut meta_theme = HashMap::new();
        meta_theme.insert("name".into(), "Thème du panneau".into());
        meta_theme.insert(
            "description".into(),
            "Sombre, clair, ou contraste renforcé pour vision basse. « Système » suit le réglage de l'OS."
                .into(),
        );
        items.push(PaletteItem {
            id: "setting_theme".into(),
            title: format!("Thème : {}", config.ui.theme.display_name()),
            subtitle: format!(
                "Validez pour passer à « {} »",
                config.ui.theme.next().display_name()
            ),
            section: PaletteSection::GeneralSettings,
            category: None,
            badge: Some(config.ui.theme.display_name().to_uppercase()),
            is_equipped: false,
            action: PaletteAction::CycleTheme,
            metadata: meta_theme,
            row_action: None,
        });

        let mut meta_motion = HashMap::new();
        meta_motion.insert("name".into(), "Animations".into());
        meta_motion.insert(
            "description".into(),
            "Le mode réduit fige le curseur de saisie, qui est une animation permanente dans le champ de vision."
                .into(),
        );
        items.push(PaletteItem {
            id: "setting_reduced_motion".into(),
            title: format!("Animations : {}", config.ui.motion_label()),
            subtitle: if config.ui.reduced_motion {
                "Curseur de saisie fixe".into()
            } else {
                "Curseur de saisie clignotant".into()
            },
            section: PaletteSection::GeneralSettings,
            category: None,
            badge: Some(
                if config.ui.reduced_motion {
                    "RÉDUIT"
                } else {
                    "COMPLET"
                }
                .into(),
            ),
            is_equipped: config.ui.reduced_motion,
            action: PaletteAction::ToggleReducedMotion,
            metadata: meta_motion,
            row_action: None,
        });

        let mut meta_focus = HashMap::new();
        meta_focus.insert("name".into(), "Fermeture automatique".into());
        meta_focus.insert(
            "description".into(),
            "Referme le panneau dès qu'il perd le focus. Désactivez-le pour le consulter en travaillant à côté."
                .into(),
        );
        items.push(PaletteItem {
            id: "setting_close_on_focus_loss".into(),
            title: "Fermer à la perte de focus".into(),
            subtitle: if config.ui.close_on_focus_loss {
                "Le panneau se referme dès qu'on clique ailleurs".into()
            } else {
                "Le panneau reste ouvert en arrière-plan".into()
            },
            section: PaletteSection::GeneralSettings,
            category: None,
            badge: Some(
                if config.ui.close_on_focus_loss {
                    "ON"
                } else {
                    "OFF"
                }
                .into(),
            ),
            is_equipped: config.ui.close_on_focus_loss,
            action: PaletteAction::ToggleCloseOnFocusLoss,
            metadata: meta_focus,
            row_action: None,
        });

        let next_scale = config.next_scale_factor();
        let mut meta_scale = HashMap::new();
        meta_scale.insert("name".into(), "Échelle de la fenêtre".into());
        meta_scale.insert(
            "description".into(),
            format!(
                "Ajuste le zoom de {}x à {}x sans flou pixel-art.",
                AppConfig::MIN_SCALE_FACTOR,
                AppConfig::MAX_SCALE_FACTOR
            ),
        );
        items.push(PaletteItem {
            id: "setting_scale".into(),
            title: format!("Échelle de zoom : {}x", config.scale_factor),
            subtitle: format!("Validez pour passer à {next_scale}x"),
            section: PaletteSection::GeneralSettings,
            category: None,
            badge: Some(format!("{}x", config.scale_factor)),
            is_equipped: false,
            action: PaletteAction::CycleScaleFactor { next: next_scale },
            metadata: meta_scale,
            row_action: None,
        });

        let mut meta_ct = HashMap::new();
        meta_ct.insert("name".into(), "Mode Click-Through".into());
        meta_ct.insert(
            "description".into(),
            "Permet aux clics souris de traverser la fenêtre transparente.".into(),
        );
        items.push(PaletteItem {
            id: "setting_click_through".into(),
            title: "Mode Click-Through (Traversant)".into(),
            subtitle: if config.click_through_enabled {
                "Actif (souris traverse Gremlin)".into()
            } else {
                "Inactif (fenêtre interactive)".into()
            },
            section: PaletteSection::GeneralSettings,
            category: None,
            badge: Some(
                if config.click_through_enabled {
                    "ON"
                } else {
                    "OFF"
                }
                .into(),
            ),
            is_equipped: config.click_through_enabled,
            action: PaletteAction::ToggleClickThrough,
            metadata: meta_ct,
            row_action: None,
        });

        let mut meta_save = HashMap::new();
        meta_save.insert("name".into(), "Sauvegarder immédiatement".into());
        meta_save.insert(
            "description".into(),
            last_save_error.map_or_else(
                || "Écrit l'état complet du familier de manière atomique.".to_owned(),
                |err| format!("Dernier échec : {err}"),
            ),
        );
        items.push(PaletteItem {
            id: "action_save_now".into(),
            title: "Sauvegarder l'état (Atomic Save)".into(),
            subtitle: last_save_error.map_or_else(
                || "Écrit save.json sur disque".to_owned(),
                |err| format!("⚠ Échec de la dernière sauvegarde : {err}"),
            ),
            section: PaletteSection::GeneralSettings,
            category: None,
            badge: Some(if last_save_error.is_some() {
                "ÉCHEC".into()
            } else {
                "PERSISTANCE".into()
            }),
            is_equipped: false,
            action: PaletteAction::SaveNow,
            metadata: meta_save,
            row_action: None,
        });

        let mut meta_reload = HashMap::new();
        meta_reload.insert("name".into(), "Recharger les Mods & Skins".into());
        meta_reload.insert(
            "description".into(),
            "Recharge immédiatement tous les manifests et fichiers PNG.".into(),
        );
        items.push(PaletteItem {
            id: "action_reload_assets".into(),
            title: "Recharger les Skins et Accessoires".into(),
            subtitle: "Actualise le dossier de configuration utilisateur".into(),
            section: PaletteSection::GeneralSettings,
            category: None,
            badge: Some("HOT-RELOAD".into()),
            is_equipped: false,
            action: PaletteAction::ReloadAssets,
            metadata: meta_reload,
            row_action: None,
        });

        let mut meta_folder = HashMap::new();
        meta_folder.insert("name".into(), "Ouvrir le dossier Skins / Mods".into());
        meta_folder.insert(
            "description".into(),
            "Ouvre le répertoire local contenant les skins et accessoires custom.".into(),
        );
        items.push(PaletteItem {
            id: "action_open_mods_folder".into(),
            title: "Ouvrir le répertoire des Skins / Mods".into(),
            subtitle: "Explorateur de fichiers OS".into(),
            section: PaletteSection::GeneralSettings,
            category: None,
            badge: Some("DOSSIER".into()),
            is_equipped: false,
            action: PaletteAction::OpenModsFolder,
            metadata: meta_folder,
            row_action: None,
        });

        let mut meta_data_folder = HashMap::new();
        meta_data_folder.insert("name".into(), "Ouvrir le dossier de Sauvegarde".into());
        meta_data_folder.insert(
            "description".into(),
            "Ouvre le répertoire contenant le fichier de sauvegarde.".into(),
        );
        items.push(PaletteItem {
            id: "action_open_data_folder".into(),
            title: "Ouvrir le répertoire des Données / Save".into(),
            subtitle: "Accéder au fichier save.json".into(),
            section: PaletteSection::GeneralSettings,
            category: None,
            badge: Some("DONNÉES".into()),
            is_equipped: false,
            action: PaletteAction::OpenDataFolder,
            metadata: meta_data_folder,
            row_action: None,
        });

        self.all_items = items;
        self.rebuild_root_items();
        self.apply_filter();
        self.clamp_selection();
    }

    /// Reconstruit les entrées de racine depuis les feuilles disponibles.
    ///
    /// Un groupe sans feuille n'apparaît pas : proposer « Dépôts surveillés »
    /// quand aucun dépôt n'a été détecté mènerait à une liste vide.
    fn rebuild_root_items(&mut self) {
        self.root_items.clear();

        for group in PaletteGroup::ALL {
            let count = self
                .all_items
                .iter()
                .filter(|item| item.section.group() == group)
                .count();
            if count == 0 {
                continue;
            }

            let mut metadata = HashMap::new();
            metadata.insert("name".into(), group.title().to_owned());
            metadata.insert("description".into(), group.subtitle().to_owned());

            self.root_items.push(PaletteItem {
                id: format!("group_{}", group.title().to_lowercase().replace(' ', "_")),
                title: group.title().to_owned(),
                subtitle: group.subtitle().to_owned(),
                section: group.sections_head(),
                category: None,
                badge: Some(count.to_string()),
                is_equipped: false,
                action: PaletteAction::EnterGroup(group),
                metadata,
                row_action: None,
            });
        }
    }

    /// Remplace la recherche et replace le curseur en fin de saisie.
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.caret = self.query.len();
        self.apply_filter();
        self.selected_index = 0;
    }

    /// Texte de recherche courant.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Position du curseur de saisie, en octets.
    #[must_use]
    pub const fn caret(&self) -> usize {
        self.caret
    }

    /// Niveau de navigation courant.
    #[must_use]
    pub const fn view(&self) -> PaletteView {
        self.view
    }

    /// Insère un caractère à la position du curseur.
    pub fn insert_char(&mut self, ch: char) {
        self.query.insert(self.caret, ch);
        self.caret += ch.len_utf8();
        self.apply_filter();
        self.selected_index = 0;
    }

    /// Supprime le caractère précédant le curseur.
    pub fn delete_before_caret(&mut self) {
        let Some((offset, _)) = self.query[..self.caret].char_indices().next_back() else {
            return;
        };
        self.query.remove(offset);
        self.caret = offset;
        self.apply_filter();
        self.clamp_selection();
    }

    /// Supprime le mot précédant le curseur.
    pub fn delete_word_before_caret(&mut self) {
        let head = &self.query[..self.caret];
        let trimmed = head.trim_end();
        let boundary = trimmed
            .rfind(char::is_whitespace)
            .map_or(0, |index| index + 1);

        self.query.replace_range(boundary..self.caret, "");
        self.caret = boundary;
        self.apply_filter();
        self.clamp_selection();
    }

    /// Efface entièrement la recherche.
    pub fn clear_query(&mut self) {
        self.query.clear();
        self.caret = 0;
        self.apply_filter();
        self.selected_index = 0;
    }

    /// Déplace le curseur d'un caractère vers la gauche.
    pub fn move_caret_left(&mut self) {
        if let Some((offset, _)) = self.query[..self.caret].char_indices().next_back() {
            self.caret = offset;
        }
    }

    /// Déplace le curseur d'un caractère vers la droite.
    pub fn move_caret_right(&mut self) {
        if let Some(ch) = self.query[self.caret..].chars().next() {
            self.caret += ch.len_utf8();
        }
    }

    /// Place le curseur au début de la saisie.
    pub const fn move_caret_to_start(&mut self) {
        self.caret = 0;
    }

    /// Place le curseur à la fin de la saisie.
    pub fn move_caret_to_end(&mut self) {
        self.caret = self.query.len();
    }

    /// Descend dans un groupe.
    ///
    /// La recherche est effacée : elle est globale par nature, la conserver en
    /// entrant dans un groupe afficherait des résultats venus d'ailleurs.
    pub fn enter_group(&mut self, group: PaletteGroup) {
        self.view = PaletteView::Group(group);
        self.query.clear();
        self.caret = 0;
        self.apply_filter();
        self.selected_index = 0;
    }

    /// Ouvre une saisie guidée : le champ de recherche devient un champ de valeur.
    pub fn enter_prompt(&mut self, kind: PromptKind) {
        self.view = PaletteView::Prompt(kind);
        self.query.clear();
        self.caret = 0;
        self.apply_filter();
        self.selected_index = 0;
    }

    /// Remonte d'un niveau, en signalant si un niveau a été quitté.
    ///
    /// La sélection est reposée sur le groupe qu'on quitte : remonter ne fait
    /// pas perdre le fil de là où l'on était.
    pub fn ascend(&mut self) -> bool {
        // Une saisie guidée redescend dans le groupe qui l'a ouverte, jamais à
        // la racine : abandonner un ajout ne doit pas renvoyer l'utilisateur
        // deux niveaux plus haut que là où il était.
        if let PaletteView::Prompt(kind) = self.view {
            self.enter_group(kind.parent_group());
            return true;
        }

        let PaletteView::Group(group) = self.view else {
            return false;
        };

        self.view = PaletteView::Root;
        self.query.clear();
        self.caret = 0;
        self.apply_filter();

        let target = self
            .root_items
            .iter()
            .position(|item| item.action == PaletteAction::EnterGroup(group));
        self.selected_index = target
            .and_then(|root| {
                self.filtered
                    .iter()
                    .position(|reference| *reference == FilteredRef::Root(root))
            })
            .unwrap_or(0);

        true
    }

    fn apply_filter(&mut self) {
        self.filtered.clear();
        let query = self.query.trim();

        // Mode saisie : le champ ne filtre plus, il porte une valeur. La liste se
        // réduit à la ligne qui rend compte de sa validité en direct.
        if let PaletteView::Prompt(kind) = self.view {
            self.prompt_item = Some(Self::build_prompt_item(kind, query));
            self.filtered.push(FilteredRef::Prompt);
            return;
        }
        self.prompt_item = None;

        if query.is_empty() {
            match self.view {
                PaletteView::Root => self
                    .filtered
                    .extend((0..self.root_items.len()).map(FilteredRef::Root)),
                PaletteView::Group(group) => self.filtered.extend(
                    self.all_items
                        .iter()
                        .enumerate()
                        .filter(|(_, item)| item.section.group() == group)
                        .map(|(index, _)| FilteredRef::Leaf(index)),
                ),
                // Traité plus haut : le mode saisie ne passe jamais ici.
                PaletteView::Prompt(_) => {}
            }
            return;
        }

        // Toute saisie bascule en recherche globale, quel que soit le niveau où
        // l'on se trouve : c'est le comportement de Raycast, et cela évite
        // d'obliger l'utilisateur à remonter pour chercher ailleurs.
        let mut scored: Vec<(PaletteSection, i32, usize)> = Vec::new();
        for (index, item) in self.all_items.iter().enumerate() {
            let fields = [
                item.title.as_str(),
                item.subtitle.as_str(),
                item.id.as_str(),
            ];
            if let Some(score) = search::best_score(&fields, query) {
                scored.push((item.section, score, index));
            }
        }

        // Les sections sont ordonnées par leur meilleur score, et les résultats
        // d'une même section restent contigus : la section la plus pertinente
        // arrive en tête, et le rendu peut poser un libellé de groupe unique.
        let mut best_by_section: BTreeMap<PaletteSection, i32> = BTreeMap::new();
        for (section, score, _) in &scored {
            let slot = best_by_section.entry(*section).or_insert(i32::MIN);
            *slot = (*slot).max(*score);
        }

        scored.sort_by(|left, right| {
            let left_group = best_by_section.get(&left.0).copied().unwrap_or(i32::MIN);
            let right_group = best_by_section.get(&right.0).copied().unwrap_or(i32::MIN);
            right_group
                .cmp(&left_group)
                .then_with(|| left.0.cmp(&right.0))
                .then_with(|| right.1.cmp(&left.1))
                .then_with(|| left.2.cmp(&right.2))
        });

        self.filtered.extend(
            scored
                .into_iter()
                .map(|(_, _, index)| FilteredRef::Leaf(index)),
        );
    }

    /// Construit la ligne de confirmation du mode saisie.
    ///
    /// La validation est refaite à chaque frappe : un seul appel système par
    /// caractère, et l'utilisateur sait immédiatement où il en est plutôt que de
    /// découvrir le refus après avoir validé.
    fn build_prompt_item(kind: PromptKind, raw: &str) -> PaletteItem {
        let verdict = RepoPathVerdict::of(raw);
        let mut metadata = HashMap::new();
        metadata.insert("name".into(), kind.breadcrumb().to_owned());
        metadata.insert("description".into(), verdict.message().to_owned());
        if !raw.is_empty() {
            metadata.insert("path".into(), raw.to_owned());
        }

        PaletteItem {
            id: "prompt_add_tracked_repo".into(),
            title: if raw.is_empty() {
                kind.hint().to_owned()
            } else {
                raw.to_owned()
            },
            subtitle: verdict.message().to_owned(),
            section: PaletteSection::GitWatcher,
            category: None,
            badge: Some(verdict.badge().to_owned()),
            is_equipped: verdict == RepoPathVerdict::Valid,
            // Une saisie invalide ne porte aucune action : valider ne produit
            // alors rien, et surtout aucun faux succès.
            action: if verdict == RepoPathVerdict::Valid {
                PaletteAction::AddTrackedRepo(PathBuf::from(raw))
            } else {
                PaletteAction::None
            },
            metadata,
            row_action: None,
        }
    }

    fn clamp_selection(&mut self) {
        self.selected_index = self
            .selected_index
            .min(self.filtered.len().saturating_sub(1));
    }

    /// Sélectionne l'élément suivant dans la liste.
    pub fn select_next(&mut self) {
        if !self.filtered.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.filtered.len();
        }
    }

    /// Sélectionne l'élément précédent dans la liste.
    pub fn select_prev(&mut self) {
        if !self.filtered.is_empty() {
            if self.selected_index == 0 {
                self.selected_index = self.filtered.len() - 1;
            } else {
                self.selected_index -= 1;
            }
        }
    }

    /// Avance la sélection d'une page, sans bouclage.
    pub fn select_page_down(&mut self, page: usize) {
        if self.filtered.is_empty() {
            return;
        }
        let last = self.filtered.len() - 1;
        self.selected_index = self.selected_index.saturating_add(page.max(1)).min(last);
    }

    /// Recule la sélection d'une page, sans bouclage.
    pub fn select_page_up(&mut self, page: usize) {
        self.selected_index = self.selected_index.saturating_sub(page.max(1));
    }

    /// Sélectionne le premier élément.
    pub const fn select_first(&mut self) {
        self.selected_index = 0;
    }

    /// Sélectionne le dernier élément.
    pub fn select_last(&mut self) {
        self.selected_index = self.filtered.len().saturating_sub(1);
    }

    /// Récupère l'élément actuellement sélectionné.
    #[must_use]
    pub fn current_selected_item(&self) -> Option<&PaletteItem> {
        self.filtered_item(self.selected_index)
    }

    /// Indice de l'élément sélectionné parmi les éléments filtrés.
    #[must_use]
    pub const fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// Indice du premier élément affiché, pour une fenêtre de `visible` lignes.
    ///
    /// Source de vérité **unique** du défilement : le moteur de rendu et le test
    /// de survol à la souris l'appellent tous deux. Dupliquer ce calcul ferait
    /// activer une ligne différente de celle que l'utilisateur a cliquée dès que
    /// les deux formules divergeraient d'une unité.
    #[must_use]
    pub const fn scroll_offset(&self, visible: usize) -> usize {
        self.selected_index
            .saturating_sub(visible.saturating_sub(1))
    }

    /// Élément affiché à la `row`-ième ligne visible, s'il existe.
    #[must_use]
    pub fn item_at_visible_row(&self, row: usize, visible: usize) -> Option<usize> {
        let index = self.scroll_offset(visible).checked_add(row)?;
        (index < self.filtered.len()).then_some(index)
    }

    /// Place la sélection sur un élément désigné, en bornant l'indice reçu.
    ///
    /// Le survol et le clic proviennent de coordonnées de souris : l'indice
    /// dérivé doit être borné plutôt que supposé valide.
    pub fn select_index(&mut self, index: usize) {
        if index < self.filtered.len() {
            self.selected_index = index;
        }
    }

    /// Nombre d'éléments retenus par le filtre courant.
    #[must_use]
    pub fn filtered_len(&self) -> usize {
        self.filtered.len()
    }

    /// Élément filtré à la position donnée.
    #[must_use]
    pub fn filtered_item(&self, index: usize) -> Option<&PaletteItem> {
        self.filtered
            .get(index)
            .and_then(|reference| self.resolve(*reference))
    }

    /// Itère sur les éléments retenus par le filtre courant.
    pub fn filtered_items(&self) -> impl Iterator<Item = &PaletteItem> + '_ {
        self.filtered
            .iter()
            .filter_map(|reference| self.resolve(*reference))
    }

    /// Résout une référence filtrée vers l'item correspondant.
    fn resolve(&self, reference: FilteredRef) -> Option<&PaletteItem> {
        match reference {
            FilteredRef::Root(index) => self.root_items.get(index),
            FilteredRef::Leaf(index) => self.all_items.get(index),
            FilteredRef::Prompt => self.prompt_item.as_ref(),
        }
    }

    /// Exécute l'action associée à l'élément actuellement sélectionné.
    ///
    /// Prend `&mut self` parce que la descente dans un groupe est une action au
    /// même titre que les autres, et qu'elle modifie l'état de navigation. La
    /// faire remonter jusqu'à l'orchestrateur pour qu'il la renvoie ensuite à la
    /// palette n'aurait fait qu'éparpiller la logique.
    #[must_use]
    pub fn execute_selected(&mut self, wardrobe: &WardrobeEquipment) -> PaletteExecutionResult {
        // L'action est copiée avant toute mutation : la navigation emprunte
        // `self` en écriture, ce qui est incompatible avec la référence prêtée
        // par `current_selected_item`.
        let Some(action) = self.current_selected_item().map(|item| item.action.clone()) else {
            return PaletteExecutionResult::None;
        };
        self.execute(&action, wardrobe)
    }

    /// Exécute l'action secondaire de la ligne sélectionnée, si elle en porte une.
    #[must_use]
    pub fn execute_row_action(&mut self, wardrobe: &WardrobeEquipment) -> PaletteExecutionResult {
        let Some(action) = self
            .current_selected_item()
            .and_then(|item| item.row_action.as_ref())
            .map(|row_action| row_action.action.clone())
        else {
            return PaletteExecutionResult::None;
        };
        self.execute(&action, wardrobe)
    }

    /// Traduit une action de palette en résultat pour l'orchestrateur.
    fn execute(
        &mut self,
        action: &PaletteAction,
        wardrobe: &WardrobeEquipment,
    ) -> PaletteExecutionResult {
        match action {
            PaletteAction::EnterGroup(group) => {
                self.enter_group(*group);
                PaletteExecutionResult::None
            }
            PaletteAction::PromptAddTrackedRepo => {
                self.enter_prompt(PromptKind::AddTrackedRepo);
                PaletteExecutionResult::None
            }
            PaletteAction::ToggleAccessory { category, id } => {
                if wardrobe.is_equipped_in(*category, id) {
                    PaletteExecutionResult::UnequipAccessory {
                        category: *category,
                    }
                } else {
                    PaletteExecutionResult::EquipAccessory {
                        category: *category,
                        id: id.clone(),
                    }
                }
            }
            PaletteAction::PetGremlin => PaletteExecutionResult::PetGremlin,
            PaletteAction::RevivePet => PaletteExecutionResult::RevivePet,
            PaletteAction::UseConsumable(kind) => PaletteExecutionResult::UseConsumable(*kind),
            PaletteAction::TogglePomodoro => PaletteExecutionResult::TogglePomodoro,
            PaletteAction::StartPomodoro => PaletteExecutionResult::StartPomodoro,
            PaletteAction::PausePomodoro => PaletteExecutionResult::PausePomodoro,
            PaletteAction::ResumePomodoro => PaletteExecutionResult::ResumePomodoro,
            PaletteAction::StopPomodoro => PaletteExecutionResult::StopPomodoro,
            PaletteAction::SkipPomodoroBreak => PaletteExecutionResult::SkipPomodoroBreak,
            PaletteAction::ToggleDesktopMotion => PaletteExecutionResult::ToggleDesktopMotion,
            PaletteAction::ToggleDesktopMagnetism => PaletteExecutionResult::ToggleDesktopMagnetism,
            PaletteAction::ToggleSleep => PaletteExecutionResult::ToggleSleep,
            PaletteAction::ToggleClickThrough => PaletteExecutionResult::ToggleClickThrough,
            PaletteAction::ToggleAutostart => PaletteExecutionResult::ToggleAutostart,
            PaletteAction::ToggleToolingWatcher => PaletteExecutionResult::ToggleToolingWatcher,
            PaletteAction::ToggleFocusTracking => PaletteExecutionResult::ToggleFocusTracking,
            PaletteAction::ToggleBreakReminders => PaletteExecutionResult::ToggleBreakReminders,
            PaletteAction::CycleScaleFactor { next } => {
                PaletteExecutionResult::SetScaleFactor(*next)
            }
            PaletteAction::ReloadAssets => PaletteExecutionResult::ReloadAssets,
            PaletteAction::OpenModsFolder => PaletteExecutionResult::OpenModsFolder,
            PaletteAction::OpenDataFolder => PaletteExecutionResult::OpenDataFolder,
            PaletteAction::SaveNow => PaletteExecutionResult::SaveNow,
            PaletteAction::CycleTextSize => PaletteExecutionResult::CycleTextSize,
            PaletteAction::CycleTheme => PaletteExecutionResult::CycleTheme,
            PaletteAction::ToggleReducedMotion => PaletteExecutionResult::ToggleReducedMotion,
            PaletteAction::ToggleCloseOnFocusLoss => PaletteExecutionResult::ToggleCloseOnFocusLoss,
            PaletteAction::AddTrackedRepo(path) => {
                PaletteExecutionResult::AddTrackedRepo(path.clone())
            }
            PaletteAction::RemoveTrackedRepo(path) => {
                PaletteExecutionResult::RemoveTrackedRepo(path.clone())
            }
            PaletteAction::OpenRepoFolder(path) => {
                PaletteExecutionResult::OpenRepoFolder(path.clone())
            }
            PaletteAction::BrowseForTrackedRepo => PaletteExecutionResult::BrowseForTrackedRepo,
            PaletteAction::None => PaletteExecutionResult::None,
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn context<'a>(
        catalog: &'a AccessoryCatalog,
        wardrobe: &'a WardrobeEquipment,
        pet: &'a PetState,
        config: &'a AppConfig,
    ) -> PaletteContext<'a> {
        PaletteContext {
            catalog,
            wardrobe,
            pet_state: pet,
            config,
            autostart_active: false,
            repos: &[],
            current_dir_repo: None,
            folder_picker_available: false,
            last_save_error: None,
            last_observation_error: None,
            pending_tooling_enabled: None,
            today: CivilDate::new(2024, 5, 10).ok(),
            desktop_placement_available: true,
            desktop_unavailable_reason: None,
        }
    }

    #[test]
    fn test_command_palette_navigation_and_filtering() {
        let catalog = AccessoryCatalog::new();
        let wardrobe = WardrobeEquipment::default();
        let pet = PetState::new("Gizmo");
        let config = AppConfig::default();
        let mut palette = CommandPalette::new(&context(&catalog, &wardrobe, &pet, &config));

        assert!(palette.filtered_len() > 0);
        assert_eq!(palette.selected_index(), 0);

        palette.select_next();
        assert_eq!(palette.selected_index(), 1);

        palette.select_prev();
        assert_eq!(palette.selected_index(), 0);

        // Nourrir passe désormais par l'inventaire : la recherche doit tomber
        // sur la collation, pas sur une action illimitée disparue.
        palette.set_query("collation");
        assert_eq!(palette.filtered_len(), 1);
        assert_eq!(
            palette.filtered_item(0).map(|i| i.id.as_str()),
            Some("inventory_snack")
        );

        let res = palette.execute_selected(&wardrobe);
        assert_eq!(
            res,
            PaletteExecutionResult::UseConsumable(ConsumableKind::Snack)
        );
    }

    #[test]
    fn test_filtering_is_accent_and_case_insensitive_on_exact_terms() {
        let catalog = AccessoryCatalog::new();
        let wardrobe = WardrobeEquipment::default();
        let pet = PetState::new("Gizmo");
        let config = AppConfig::default();
        let mut palette = CommandPalette::new(&context(&catalog, &wardrobe, &pet, &config));

        palette.set_query("SOIGNER");
        assert!(palette.filtered_len() >= 1);

        // Une requête sans résultat ne doit pas laisser une sélection invalide.
        palette.set_query("zzzzzzzz-introuvable");
        assert_eq!(palette.filtered_len(), 0);
        assert!(palette.current_selected_item().is_none());
        assert_eq!(
            palette.execute_selected(&wardrobe),
            PaletteExecutionResult::None
        );
    }

    #[test]
    fn test_scale_cycle_reads_config_and_wraps_at_the_maximum() {
        let catalog = AccessoryCatalog::new();
        let wardrobe = WardrobeEquipment::default();
        let pet = PetState::new("Gizmo");

        for scale in AppConfig::MIN_SCALE_FACTOR..=AppConfig::MAX_SCALE_FACTOR {
            let config = AppConfig {
                scale_factor: scale,
                ..AppConfig::default()
            };
            let mut palette = CommandPalette::new(&context(&catalog, &wardrobe, &pet, &config));
            palette.set_query("échelle de zoom");

            let expected = if scale >= AppConfig::MAX_SCALE_FACTOR {
                AppConfig::MIN_SCALE_FACTOR
            } else {
                scale + 1
            };
            assert_eq!(
                palette.execute_selected(&wardrobe),
                PaletteExecutionResult::SetScaleFactor(expected),
                "cycle incorrect depuis l'échelle {scale}x"
            );
        }
    }

    #[test]
    fn test_save_failure_is_surfaced_to_the_user() {
        let catalog = AccessoryCatalog::new();
        let wardrobe = WardrobeEquipment::default();
        let pet = PetState::new("Gizmo");
        let config = AppConfig::default();

        let mut ctx = context(&catalog, &wardrobe, &pet, &config);
        ctx.last_save_error = Some("disque plein");
        let mut palette = CommandPalette::new(&ctx);
        palette.set_query("sauvegarder");

        let item = palette
            .filtered_item(0)
            .expect("l'item de sauvegarde doit exister");
        assert_eq!(item.badge.as_deref(), Some("ÉCHEC"));
        assert!(item.subtitle.contains("disque plein"));
    }

    #[test]
    fn test_critical_gauge_takes_over_the_profile_badge() {
        let catalog = AccessoryCatalog::new();
        let wardrobe = WardrobeEquipment::default();
        let config = AppConfig::default();

        // La racine n'énumère plus que les groupes : il faut descendre dans
        // « Profil du Gremlin » pour atteindre la feuille qui porte le badge.
        let healthy = PetState::new("Gizmo");
        let mut palette = CommandPalette::new(&context(&catalog, &wardrobe, &healthy, &config));
        palette.enter_group(PaletteGroup::Profile);
        let badge = palette
            .filtered_item(0)
            .and_then(|item| item.badge.clone())
            .expect("le profil porte toujours un badge");
        assert_ne!(badge, "CRITIQUE");

        let mut critical = PetState::new("Gizmo");
        critical.set_stats(gremlin_core::PetStats::new(5.0, 60.0, 60.0));
        let mut palette = CommandPalette::new(&context(&catalog, &wardrobe, &critical, &config));
        palette.enter_group(PaletteGroup::Profile);
        let item = palette.filtered_item(0).expect("item de profil");
        assert_eq!(item.badge.as_deref(), Some("CRITIQUE"));
        assert!(item
            .metadata
            .get("description")
            .is_some_and(|d| d.contains("jauge vitale")));
    }

    #[test]
    fn test_revive_entry_appears_only_for_a_dead_pet() {
        let catalog = AccessoryCatalog::new();
        let wardrobe = WardrobeEquipment::default();
        let config = AppConfig::default();

        let alive = PetState::new("Gizmo");
        let mut palette = CommandPalette::new(&context(&catalog, &wardrobe, &alive, &config));
        palette.set_query("réanimer");
        assert_eq!(palette.filtered_len(), 0);

        let mut dead = PetState::new("Gizmo");
        dead.set_stats(gremlin_core::PetStats::new(0.0, 0.0, 0.0));
        let mut palette = CommandPalette::new(&context(&catalog, &wardrobe, &dead, &config));
        palette.set_query("réanimer");
        assert_eq!(palette.filtered_len(), 1);
        assert_eq!(
            palette.execute_selected(&wardrobe),
            PaletteExecutionResult::RevivePet
        );
    }

    /// Palette avec une liste de dépôts déclarés.
    fn palette_with_repos(
        catalog: &AccessoryCatalog,
        wardrobe: &WardrobeEquipment,
        pet: &PetState,
        config: &AppConfig,
        repos: &[RepoDisplayInfo],
    ) -> CommandPalette {
        CommandPalette::new(&PaletteContext {
            repos,
            ..context(catalog, wardrobe, pet, config)
        })
    }

    /// Dépôt déclaré factice.
    fn fake_repo(name: &str) -> RepoDisplayInfo {
        let path = if cfg!(windows) {
            PathBuf::from(format!(r"C:\depots\{name}"))
        } else {
            PathBuf::from(format!("/depots/{name}"))
        };
        RepoDisplayInfo::declared(path, RepoTrackingStatus::Active, None)
    }

    #[test]
    fn test_repos_group_survives_an_empty_repo_list() {
        // L'action d'ajout est permanente : sans elle, le groupe disparaîtrait
        // de la racine et l'état vide n'aurait nulle part où s'expliquer.
        let catalog = AccessoryCatalog::new();
        let wardrobe = WardrobeEquipment::default();
        let pet = PetState::new("Gizmo");
        let config = AppConfig::default();

        let mut palette = CommandPalette::new(&context(&catalog, &wardrobe, &pet, &config));
        assert!(palette
            .filtered_items()
            .any(|item| item.action == PaletteAction::EnterGroup(PaletteGroup::Repos)));

        palette.enter_group(PaletteGroup::Repos);
        assert_eq!(palette.filtered_len(), 1);
        let item = palette.filtered_item(0).expect("action d'ajout");
        assert_eq!(item.action, PaletteAction::PromptAddTrackedRepo);
        assert!(item.subtitle.contains("Aucun dépôt surveillé"));
    }

    #[test]
    fn test_repo_rows_carry_a_named_removal_action() {
        let catalog = AccessoryCatalog::new();
        let wardrobe = WardrobeEquipment::default();
        let pet = PetState::new("Gizmo");
        let config = AppConfig::default();
        let repos = [fake_repo("alpha")];

        let mut palette = palette_with_repos(&catalog, &wardrobe, &pet, &config, &repos);
        palette.enter_group(PaletteGroup::Repos);

        let row = (0..palette.filtered_len())
            .find(|index| {
                palette
                    .filtered_item(*index)
                    .is_some_and(|item| item.row_action.is_some())
            })
            .expect("la ligne du dépôt doit porter une action");
        palette.select_index(row);

        let item = palette.filtered_item(row).expect("ligne de dépôt");
        let action = item.row_action.as_ref().expect("action de ligne");
        assert_eq!(action.icon, RowActionIcon::Trash);
        assert_eq!(action.label, "Retirer alpha de la surveillance");

        // Valider la ligne ouvre le dossier ; le bouton, lui, retire le dépôt.
        assert_eq!(
            palette.execute_selected(&wardrobe),
            PaletteExecutionResult::OpenRepoFolder(repos[0].path.clone())
        );
        assert_eq!(
            palette.execute_row_action(&wardrobe),
            PaletteExecutionResult::RemoveTrackedRepo(repos[0].path.clone())
        );
    }

    #[test]
    fn test_rows_without_action_ignore_the_row_action_gesture() {
        let catalog = AccessoryCatalog::new();
        let wardrobe = WardrobeEquipment::default();
        let pet = PetState::new("Gizmo");
        let config = AppConfig::default();

        let mut palette = CommandPalette::new(&context(&catalog, &wardrobe, &pet, &config));
        palette.enter_group(PaletteGroup::Preferences);

        assert_eq!(
            palette.execute_row_action(&wardrobe),
            PaletteExecutionResult::None,
            "la touche de retrait ne doit rien déclencher hors des dépôts"
        );
    }

    #[test]
    fn test_prompt_mode_reports_the_verdict_of_the_typed_path() {
        let catalog = AccessoryCatalog::new();
        let wardrobe = WardrobeEquipment::default();
        let pet = PetState::new("Gizmo");
        let config = AppConfig::default();

        let mut palette = CommandPalette::new(&context(&catalog, &wardrobe, &pet, &config));
        palette.enter_prompt(PromptKind::AddTrackedRepo);

        // Saisie vide : une seule ligne, en attente, sans action.
        assert_eq!(palette.filtered_len(), 1);
        let item = palette.filtered_item(0).expect("ligne de confirmation");
        assert_eq!(item.badge.as_deref(), Some("EN ATTENTE"));
        assert_eq!(item.action, PaletteAction::None);

        // Chemin relatif : refusé, et le motif est dit.
        palette.set_query("projet_relatif");
        let item = palette.filtered_item(0).expect("ligne de confirmation");
        assert_eq!(item.badge.as_deref(), Some("INVALIDE"));
        assert!(item.subtitle.contains("chemin absolu"));
        assert_eq!(
            palette.execute_selected(&wardrobe),
            PaletteExecutionResult::None,
            "valider une saisie invalide ne doit produire aucun faux succès"
        );

        // Chemin absolu qui n'est pas un dépôt : refusé aussi.
        palette.set_query(if cfg!(windows) {
            r"C:\chemin\qui\nexiste\pas"
        } else {
            "/chemin/qui/nexiste/pas"
        });
        let item = palette.filtered_item(0).expect("ligne de confirmation");
        assert_eq!(item.badge.as_deref(), Some("INVALIDE"));
        assert!(item.subtitle.contains("dépôt Git valide"));
    }

    #[test]
    fn test_prompt_mode_accepts_a_real_repository() {
        let catalog = AccessoryCatalog::new();
        let wardrobe = WardrobeEquipment::default();
        let pet = PetState::new("Gizmo");
        let config = AppConfig::default();

        // Le dépôt doit exister sur le disque : c'est tout l'intérêt de la
        // validation en direct.
        let root = std::env::temp_dir().join(format!(
            "gremlin-palette-prompt-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(root.join(".git")).expect("dépôt de test");

        let mut palette = CommandPalette::new(&context(&catalog, &wardrobe, &pet, &config));
        palette.enter_prompt(PromptKind::AddTrackedRepo);
        palette.set_query(root.to_string_lossy().as_ref());

        let item = palette.filtered_item(0).expect("ligne de confirmation");
        assert_eq!(item.badge.as_deref(), Some("VALIDE"));
        assert_eq!(
            palette.execute_selected(&wardrobe),
            PaletteExecutionResult::AddTrackedRepo(root.clone())
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_leaving_the_prompt_returns_to_the_repos_group() {
        // Abandonner un ajout ne doit pas renvoyer deux niveaux plus haut.
        let catalog = AccessoryCatalog::new();
        let wardrobe = WardrobeEquipment::default();
        let pet = PetState::new("Gizmo");
        let config = AppConfig::default();

        let mut palette = CommandPalette::new(&context(&catalog, &wardrobe, &pet, &config));
        palette.enter_group(PaletteGroup::Repos);
        palette.enter_prompt(PromptKind::AddTrackedRepo);
        assert!(palette.view().is_prompt());

        assert!(palette.ascend());
        assert_eq!(palette.view(), PaletteView::Group(PaletteGroup::Repos));
        assert!(palette.query().is_empty());

        assert!(palette.ascend());
        assert_eq!(palette.view(), PaletteView::Root);
    }

    #[test]
    fn test_folder_browsing_entry_appears_only_when_supported() {
        let catalog = AccessoryCatalog::new();
        let wardrobe = WardrobeEquipment::default();
        let pet = PetState::new("Gizmo");
        let config = AppConfig::default();

        let has_browse = |available: bool| {
            let mut palette = CommandPalette::new(&PaletteContext {
                folder_picker_available: available,
                ..context(&catalog, &wardrobe, &pet, &config)
            });
            palette.enter_group(PaletteGroup::Repos);
            (0..palette.filtered_len()).any(|index| {
                palette
                    .filtered_item(index)
                    .is_some_and(|item| item.action == PaletteAction::BrowseForTrackedRepo)
            })
        };

        // Une commande qui échouerait systématiquement vaut moins qu'une
        // commande absente : la saisie du chemin reste disponible partout.
        assert!(has_browse(true));
        assert!(!has_browse(false));
    }
}
