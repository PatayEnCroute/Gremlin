//! Moteur de recherche et logique de la palette de commande style Raycast.

use crate::config::AppConfig;
use gremlin_core::PetState;
use gremlin_render::{AccessoryCatalog, AccessoryCategory, WardrobeEquipment};
use std::collections::HashMap;

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
            Self::GeneralSettings => "PREFERENCES SYSTEME",
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
}

/// État de la palette de commande et gestionnaire de filtrage.
#[derive(Debug, Clone)]
pub struct CommandPalette {
    query: String,
    selected_index: usize,
    all_items: Vec<PaletteItem>,
    /// Indices dans `all_items` retenus par le filtre courant.
    ///
    /// Stocker des indices plutôt que des copies évite de cloner la totalité
    /// des items — `HashMap` de métadonnées comprises — à chaque frappe.
    filtered_indices: Vec<usize>,
}

impl CommandPalette {
    /// Crée une nouvelle palette de commande initialisée avec tous les éléments disponibles.
    #[must_use]
    pub fn new(context: &PaletteContext<'_>) -> Self {
        let mut palette = Self {
            query: String::new(),
            selected_index: 0,
            all_items: Vec::new(),
            filtered_indices: Vec::new(),
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
            badge: Some(format!("+{feed_amount:.0} SATIETE")),
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
            badge: Some(if is_sleeping { "DORT" } else { "EVEILLE" }.into()),
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
                badge: Some("RESURRECTION".into()),
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
                    Some(String::from("EQUIPE"))
                } else if item.is_procedural {
                    Some(String::from("INTEGRE"))
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
                "ECHEC".into()
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
            badge: Some("DONNEES".into()),
            is_equipped: false,
            action: PaletteAction::OpenDataFolder,
            metadata: meta_data_folder,
        });

        self.all_items = items;
        self.apply_filter();
        self.clamp_selection();
    }

    /// Filtre les items en fonction du texte de recherche.
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.apply_filter();
        self.selected_index = 0;
    }

    /// Texte de recherche courant.
    #[must_use]
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Ajoute un caractère à la recherche.
    pub fn push_char(&mut self, ch: char) {
        self.query.push(ch);
        self.apply_filter();
        self.selected_index = 0;
    }

    /// Supprime le dernier caractère de la recherche.
    pub fn pop_char(&mut self) {
        self.query.pop();
        self.apply_filter();
        self.clamp_selection();
    }

    fn apply_filter(&mut self) {
        let q = self.query.trim().to_lowercase();
        self.filtered_indices.clear();

        if q.is_empty() {
            self.filtered_indices.extend(0..self.all_items.len());
            return;
        }

        for (idx, item) in self.all_items.iter().enumerate() {
            if item.title.to_lowercase().contains(&q)
                || item.subtitle.to_lowercase().contains(&q)
                || item.id.to_lowercase().contains(&q)
            {
                self.filtered_indices.push(idx);
            }
        }
    }

    fn clamp_selection(&mut self) {
        self.selected_index = self
            .selected_index
            .min(self.filtered_indices.len().saturating_sub(1));
    }

    /// Sélectionne l'élément suivant dans la liste.
    pub fn select_next(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.filtered_indices.len();
        }
    }

    /// Sélectionne l'élément précédent dans la liste.
    pub fn select_prev(&mut self) {
        if !self.filtered_indices.is_empty() {
            if self.selected_index == 0 {
                self.selected_index = self.filtered_indices.len() - 1;
            } else {
                self.selected_index -= 1;
            }
        }
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

    /// Nombre d'éléments retenus par le filtre courant.
    #[must_use]
    pub fn filtered_len(&self) -> usize {
        self.filtered_indices.len()
    }

    /// Élément filtré à la position donnée.
    #[must_use]
    pub fn filtered_item(&self, index: usize) -> Option<&PaletteItem> {
        self.filtered_indices
            .get(index)
            .and_then(|&idx| self.all_items.get(idx))
    }

    /// Itère sur les éléments retenus par le filtre courant.
    pub fn filtered_items(&self) -> impl Iterator<Item = &PaletteItem> + '_ {
        self.filtered_indices
            .iter()
            .filter_map(|&idx| self.all_items.get(idx))
    }

    /// Exécute l'action associée à l'élément actuellement sélectionné.
    #[must_use]
    pub fn execute_selected(&self, wardrobe: &WardrobeEquipment) -> PaletteExecutionResult {
        let Some(item) = self.current_selected_item() else {
            return PaletteExecutionResult::None;
        };

        match &item.action {
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
            PaletteAction::CycleScaleFactor { next } => {
                PaletteExecutionResult::SetScaleFactor(*next)
            }
            PaletteAction::ReloadAssets => PaletteExecutionResult::ReloadAssets,
            PaletteAction::OpenModsFolder => PaletteExecutionResult::OpenModsFolder,
            PaletteAction::OpenDataFolder => PaletteExecutionResult::OpenDataFolder,
            PaletteAction::SaveNow => PaletteExecutionResult::SaveNow,
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
        assert_eq!(item.badge.as_deref(), Some("ECHEC"));
        assert!(item.subtitle.contains("disque plein"));
    }

    #[test]
    fn test_critical_gauge_takes_over_the_profile_badge() {
        let catalog = AccessoryCatalog::new();
        let wardrobe = WardrobeEquipment::default();
        let config = AppConfig::default();

        let healthy = PetState::new("Gizmo");
        let palette = CommandPalette::new(&context(&catalog, &wardrobe, &healthy, &config));
        let badge = palette
            .filtered_item(0)
            .and_then(|item| item.badge.clone())
            .expect("le profil porte toujours un badge");
        assert_ne!(badge, "CRITIQUE");

        let mut critical = PetState::new("Gizmo");
        critical.set_stats(gremlin_core::PetStats::new(5.0, 60.0, 60.0));
        let palette = CommandPalette::new(&context(&catalog, &wardrobe, &critical, &config));
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
