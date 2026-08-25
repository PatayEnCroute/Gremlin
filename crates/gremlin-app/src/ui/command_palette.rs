//! Moteur de recherche et logique de la palette de commande style Raycast.

use crate::config::AppConfig;
use crate::ui::search;
use gremlin_core::PetState;
use gremlin_render::{AccessoryCatalog, AccessoryCategory, WardrobeEquipment};
use std::collections::{BTreeMap, HashMap};

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
            Self::GeneralSettings => PaletteGroup::Preferences,
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
    /// Préférences système et actions de maintenance.
    Preferences,
}

impl PaletteGroup {
    /// Tous les groupes, dans l'ordre d'affichage à la racine.
    pub const ALL: [Self; 5] = [
        Self::Profile,
        Self::Care,
        Self::Wardrobe,
        Self::Repos,
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
            Self::Preferences => "Démarrage, échelle, dossiers, sauvegarde",
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
}

impl PaletteView {
    /// Fil d'Ariane affiché en tête de la barre de recherche.
    #[must_use]
    pub const fn breadcrumb(self) -> Option<&'static str> {
        match self {
            Self::Root => None,
            Self::Group(group) => Some(group.title()),
        }
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
    /// Nourrit le Gremlin d'une collation.
    FeedPet,
    /// Caresse le Gremlin pour augmenter son bonheur.
    PetGremlin,
    /// Soigne le Gremlin en cas de maladie.
    HealPet,
    /// Réanime un Gremlin décédé.
    RevivePet,
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
    /// Action sans effet direct.
    None,
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
    /// Nourrit le familier.
    FeedPet,
    /// Caresse le familier.
    PetGremlin,
    /// Soigne le familier.
    HealPet,
    /// Réanime le familier.
    RevivePet,
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
    /// Aucune action.
    None,
}

/// Informations sommaires d'un dépôt Git surveillé pour l'affichage Raycast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoDisplayInfo {
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
        )
    }
}

impl RepoDisplayInfo {
    /// Libellé de branche affichable, y compris lorsqu'elle est inconnue.
    #[must_use]
    pub fn branch_label(&self) -> &str {
        self.branch.as_deref().unwrap_or("inconnue")
    }
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
    /// Dépôts Git actuellement surveillés.
    pub repos: &'a [RepoDisplayInfo],
    /// Dernière erreur de sauvegarde à signaler à l'utilisateur.
    pub last_save_error: Option<&'a str>,
    /// Dernier incident du watcher ou du moniteur d'activité.
    pub last_observation_error: Option<&'a str>,
    /// État d'outillage demandé, tant que le worker ne l'a pas confirmé.
    pub pending_tooling_enabled: Option<bool>,
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
            last_save_error,
            last_observation_error,
            pending_tooling_enabled,
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
        });

        // 2. Soins et actions sur le familier
        let feed_amount = core_actions.default_feed_amount;
        let mut meta_feed = HashMap::new();
        meta_feed.insert("name".into(), "Nourrir Gremlin".into());
        meta_feed.insert(
            "description".into(),
            format!("Donne une friandise pour restaurer la satiété (+{feed_amount:.0})."),
        );
        items.push(PaletteItem {
            id: "care_feed".into(),
            title: "Nourrir d'un snack".into(),
            subtitle: format!("Satiété actuelle : {:.0}%", stats.satiety()),
            section: PaletteSection::PetCare,
            category: None,
            badge: Some(format!("+{feed_amount:.0} SATIÉTÉ")),
            is_equipped: false,
            action: PaletteAction::FeedPet,
            metadata: meta_feed,
        });

        let heal_amount = core_actions.default_heal_amount;
        let mut meta_heal = HashMap::new();
        meta_heal.insert("name".into(), "Soigner Gremlin".into());
        meta_heal.insert(
            "description".into(),
            format!("Administre des soins et potions au Gremlin (+{heal_amount:.0})."),
        );
        items.push(PaletteItem {
            id: "care_heal".into(),
            title: "Soigner le familier".into(),
            subtitle: format!(
                "Bonheur : {:.0}% • Énergie : {:.0}%",
                stats.happiness(),
                stats.energy()
            ),
            section: PaletteSection::PetCare,
            category: None,
            badge: Some("SOIN".into()),
            is_equipped: false,
            action: PaletteAction::HealPet,
            metadata: meta_heal,
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
                } else if item.is_procedural {
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
                });
            }
        }

        // 4. Surveillance des dépôts Git
        for repo in repos {
            let mut meta_repo = HashMap::new();
            meta_repo.insert("name".into(), repo.name.clone());
            meta_repo.insert("branch".into(), repo.branch_label().to_owned());
            if let Some(ref msg) = repo.last_commit_msg {
                meta_repo.insert("last_commit".into(), msg.clone());
            }

            items.push(PaletteItem {
                id: format!("repo_{}", repo.name),
                title: format!("Dépôt : {}", repo.name),
                subtitle: format!(
                    "Branche : {} • Dernier commit : {}",
                    repo.branch_label(),
                    repo.last_commit_msg.as_deref().unwrap_or("aucun")
                ),
                section: PaletteSection::GitWatcher,
                category: None,
                badge: Some(repo.branch_label().to_owned()),
                is_equipped: false,
                action: PaletteAction::None,
                metadata: meta_repo,
            });
        }

        // 5. Actions et préférences système
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

    /// Remonte d'un niveau, en signalant si un niveau a été quitté.
    ///
    /// La sélection est reposée sur le groupe qu'on quitte : remonter ne fait
    /// pas perdre le fil de là où l'on était.
    pub fn ascend(&mut self) -> bool {
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

        match &action {
            PaletteAction::EnterGroup(group) => {
                self.enter_group(*group);
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
            PaletteAction::FeedPet => PaletteExecutionResult::FeedPet,
            PaletteAction::PetGremlin => PaletteExecutionResult::PetGremlin,
            PaletteAction::HealPet => PaletteExecutionResult::HealPet,
            PaletteAction::RevivePet => PaletteExecutionResult::RevivePet,
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
            last_save_error: None,
            last_observation_error: None,
            pending_tooling_enabled: None,
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

        palette.set_query("nourrir");
        assert_eq!(palette.filtered_len(), 1);
        assert_eq!(
            palette.filtered_item(0).map(|i| i.id.as_str()),
            Some("care_feed")
        );

        let res = palette.execute_selected(&wardrobe);
        assert_eq!(res, PaletteExecutionResult::FeedPet);
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
}
