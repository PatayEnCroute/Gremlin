//! Arbre d'accessibilité du panneau, exposé à l'OS via AccessKit.
//!
//! Ce module n'existe que si la feature `a11y` est activée — ce qui est le cas
//! par défaut. `cargo build --no-default-features` produit un binaire sans lui,
//! et donc sans pile UI Automation, `NSAccessibility` ni AT-SPI.
//!
//! # Ce que ce module rend possible
//!
//! Le panneau est dessiné pixel par pixel dans un tampon : pour un lecteur
//! d'écran, il n'était donc rigoureusement rien — une fenêtre opaque et muette.
//! AccessKit demande une description *sémantique* de l'interface, indépendante
//! de son dessin, et la traduit vers l'interface d'accessibilité de chaque
//! système : UI Automation sous Windows, `NSAccessibility` sous macOS, AT-SPI sous
//! Linux.
//!
//! # Pourquoi la construction est une fonction pure
//!
//! [`tree_update`] ne prend qu'un état de palette et rend un [`TreeUpdate`].
//! Elle n'ouvre aucune fenêtre, ne touche à aucun adaptateur, et se teste donc
//! intégralement sans écran ni lecteur d'écran installé — ce qui vaut mieux que
//! de découvrir en revue manuelle qu'une bascule n'annonçait pas son état.
//!
//! # Forme de l'arbre
//!
//! ```text
//! Window « Paramètres Gremlin »
//! ├── TextInput « Recherche »          valeur = texte saisi
//! └── ListBox  « Commandes » | « Garde-robe » | …
//!     ├── ListItem « Chapeau de Mage » description = sous-titre + statut
//!     ├── Switch   « Mode Click-Through » toggled = actif
//!     └── …
//! ```
//!
//! Le focus est placé sur la ligne sélectionnée, et non sur la liste : c'est ce
//! qui fait annoncer chaque déplacement au clavier.

use crate::ui::command_palette::{CommandPalette, PaletteItem};
use accesskit::{Action, Node, NodeId, Role, Toggled, Tree, TreeId, TreeUpdate};

/// Identifiant du nœud racine, la fenêtre.
pub const ROOT_ID: NodeId = NodeId(0);

/// Identifiant du champ de recherche.
pub const SEARCH_ID: NodeId = NodeId(1);

/// Identifiant de la liste.
pub const LIST_ID: NodeId = NodeId(2);

/// Premier identifiant attribué à une ligne de la liste.
///
/// Décalé pour laisser de la place à d'éventuels nœuds fixes supplémentaires
/// sans renuméroter les lignes, ce qui provoquerait des annonces parasites.
const FIRST_ROW_ID: u64 = 16;

/// Nombre d'identifiants réservés par ligne.
///
/// Une ligne peut porter un bouton d'action, qui est un nœud à part entière :
/// sans identifiant propre, il n'existerait pas pour un lecteur d'écran et la
/// commande ne serait accessible qu'à la souris.
const IDS_PER_ROW: u64 = 2;

/// Nombre maximal de lignes exposées à l'arbre.
///
/// Un lecteur d'écran parcourt la liste entière, pas seulement la portion
/// visible : toutes les lignes filtrées sont donc exposées. Le plafond ne protège
/// que d'un cas pathologique — plusieurs milliers de dépôts détectés — où la
/// construction de l'arbre deviendrait coûteuse à chaque frappe.
const MAX_EXPOSED_ROWS: usize = 500;

/// Identifiant du nœud de la `index`-ième ligne filtrée.
#[must_use]
pub const fn row_id(index: usize) -> NodeId {
    NodeId(FIRST_ROW_ID + index as u64 * IDS_PER_ROW)
}

/// Identifiant du bouton d'action de la `index`-ième ligne filtrée.
#[must_use]
pub const fn row_action_id(index: usize) -> NodeId {
    NodeId(FIRST_ROW_ID + index as u64 * IDS_PER_ROW + 1)
}

/// Indice de ligne correspondant à un identifiant de nœud, s'il en désigne une.
///
/// Le bouton d'action d'une ligne renvoie l'indice de **sa** ligne : activer le
/// bouton et activer la ligne visent la même entrée, seule l'action diffère.
#[must_use]
pub fn row_index(id: NodeId) -> Option<usize> {
    id.0.checked_sub(FIRST_ROW_ID)
        .map(|offset| (offset / IDS_PER_ROW) as usize)
}

/// Indique si l'identifiant désigne un bouton d'action plutôt que sa ligne.
#[must_use]
pub fn is_row_action(id: NodeId) -> bool {
    id.0.checked_sub(FIRST_ROW_ID)
        .is_some_and(|offset| offset % IDS_PER_ROW == 1)
}

/// Construit l'arbre complet décrivant l'état courant du panneau.
///
/// L'arbre est reconstruit intégralement à chaque appel plutôt que par delta :
/// le panneau compte quelques dizaines de nœuds, et un arbre complet interdit
/// toute divergence entre ce qui est dessiné et ce qui est annoncé.
#[must_use]
pub fn tree_update(palette: &CommandPalette) -> TreeUpdate {
    let mut nodes = Vec::with_capacity(palette.filtered_len().min(MAX_EXPOSED_ROWS) + 3);

    let exposed = palette.filtered_len().min(MAX_EXPOSED_ROWS);
    let row_ids: Vec<NodeId> = (0..exposed).map(row_id).collect();

    // --- champ de recherche ------------------------------------------------
    let mut search = Node::new(Role::TextInput);
    search.set_label("Recherche");
    search.set_value(palette.query());
    search.set_description(describe_search(palette));
    search.add_action(Action::Focus);
    search.add_action(Action::SetValue);
    nodes.push((SEARCH_ID, search));

    // --- liste --------------------------------------------------------------
    let mut list = Node::new(Role::ListBox);
    list.set_label(palette.view().breadcrumb().unwrap_or("Commandes"));
    list.set_description(describe_count(palette.filtered_len()));
    list.set_children(row_ids.clone());
    nodes.push((LIST_ID, list));

    // --- lignes -------------------------------------------------------------
    for (index, id) in row_ids.iter().enumerate() {
        let Some(item) = palette.filtered_item(index) else {
            continue;
        };
        nodes.push((*id, build_row(item, index)));

        // Le bouton d'action est un nœud enfant, avec son propre libellé : sans
        // lui, retirer un dépôt resterait une commande réservée à la souris.
        if let Some(action) = item.row_action.as_ref() {
            let mut button = Node::new(Role::Button);
            button.set_label(action.label.clone());
            button.add_action(Action::Click);
            button.add_action(Action::Focus);
            nodes.push((row_action_id(index), button));
        }
    }

    // --- racine -------------------------------------------------------------
    let mut root = Node::new(Role::Window);
    root.set_label("Paramètres Gremlin");
    root.set_children([SEARCH_ID, LIST_ID]);
    nodes.push((ROOT_ID, root));

    // Le focus suit la sélection, ce qui fait annoncer chaque déplacement au
    // clavier. Liste vide : il revient au champ de recherche, seul élément
    // encore actionnable.
    let focus = if exposed == 0 {
        SEARCH_ID
    } else {
        row_id(palette.selected_index().min(exposed - 1))
    };

    TreeUpdate {
        nodes,
        tree: Some(Tree::new(ROOT_ID)),
        tree_id: TreeId::ROOT,
        focus,
    }
}

/// Construit le nœud d'une ligne.
fn build_row(item: &PaletteItem, index: usize) -> Node {
    // Une bascule est annoncée comme telle, avec son état : un lecteur d'écran
    // dira « activé » ou « désactivé » au lieu de laisser l'utilisateur deviner
    // ce que fait la validation.
    let mut node = if item.is_toggle() {
        let mut node = Node::new(Role::Switch);
        node.set_toggled(if item.is_equipped {
            Toggled::True
        } else {
            Toggled::False
        });
        node
    } else if item.is_command_button() {
        Node::new(Role::Button)
    } else {
        Node::new(Role::ListItem)
    };

    node.set_label(item.title.clone());
    node.set_description(describe_row(item));
    // Une ligne informative n'annonce pas d'activation : promettre un clic qui
    // ne fait rien est pire que de ne rien promettre.
    if !item.is_informational() {
        node.add_action(Action::Click);
    }
    node.add_action(Action::Focus);
    node.add_action(Action::ScrollIntoView);
    if item.row_action.is_some() {
        node.set_children([row_action_id(index)]);
    }

    node
}

/// Description parlée d'une ligne : sous-titre, statut, et nature si besoin.
fn describe_row(item: &PaletteItem) -> String {
    let mut parts = Vec::with_capacity(3);

    if !item.subtitle.is_empty() {
        parts.push(item.subtitle.clone());
    }
    if let Some(badge) = &item.badge {
        parts.push(badge.clone());
    }
    if item.is_group_entry() {
        // Sans cette mention, rien ne distingue à l'oreille une entrée qui ouvre
        // une sous-liste d'une commande qui agit immédiatement.
        parts.push(String::from("ouvre une sous-liste"));
    }
    if item.row_action.is_some() {
        parts.push(String::from("touche Suppr pour retirer"));
    }
    if item.is_informational() {
        // Sans cette mention, rien ne distingue à l'oreille une ligne de lecture
        // d'une commande dont la validation resterait sans effet visible.
        parts.push(String::from("information"));
    }

    parts.join(" — ")
}

/// Description parlée du champ de recherche, contexte de navigation compris.
fn describe_search(palette: &CommandPalette) -> String {
    let count = describe_count(palette.filtered_len());
    palette.view().breadcrumb().map_or_else(
        || format!("Recherche un accessoire, un dépôt ou un réglage. {count}"),
        |group| format!("Filtre dans « {group} », ou recherche dans tout le panneau. {count}"),
    )
}

/// Formule le décompte de résultats, au singulier comme au pluriel.
fn describe_count(total: usize) -> String {
    match total {
        0 => String::from("Aucun résultat."),
        1 => String::from("1 résultat."),
        many => format!("{many} résultats."),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::ui::command_palette::{
        PaletteContext, PaletteGroup, RepoDisplayInfo, RepoTrackingStatus,
    };

    /// Chemin absolu de test, valide sur les trois systèmes.
    fn test_repo_path(name: &str) -> std::path::PathBuf {
        if cfg!(windows) {
            std::path::PathBuf::from(format!(r"C:\depots\{name}"))
        } else {
            std::path::PathBuf::from(format!("/depots/{name}"))
        }
    }
    use gremlin_core::PetState;
    use gremlin_render::register_default_accessories;
    use gremlin_render::{AccessoryCatalog, SpriteAtlas, WardrobeEquipment};

    struct Harness {
        catalog: AccessoryCatalog,
        wardrobe: WardrobeEquipment,
        pet: PetState,
        config: AppConfig,
    }

    impl Harness {
        fn new() -> Self {
            let mut atlas = SpriteAtlas::new();
            atlas.load_default_procedural_sprites();
            let mut catalog = AccessoryCatalog::new();
            register_default_accessories(&mut atlas, &mut catalog);

            Self {
                catalog,
                wardrobe: WardrobeEquipment::new(),
                pet: PetState::new("Gizmo"),
                config: AppConfig::default(),
            }
        }

        fn palette(&self, repos: &[RepoDisplayInfo]) -> CommandPalette {
            CommandPalette::new(&PaletteContext {
                catalog: &self.catalog,
                wardrobe: &self.wardrobe,
                pet_state: &self.pet,
                config: &self.config,
                autostart_active: false,
                repos,
                current_dir_repo: None,
                folder_picker_available: false,
                last_save_error: None,
                last_observation_error: None,
                pending_tooling_enabled: None,
                today: gremlin_core::CivilDate::new(2024, 5, 10).ok(),
                desktop_placement_available: true,
                desktop_unavailable_reason: None,
            })
        }
    }

    fn node_of(update: &TreeUpdate, id: NodeId) -> &Node {
        update
            .nodes
            .iter()
            .find(|(node_id, _)| *node_id == id)
            .map(|(_, node)| node)
            .expect("nœud absent de l'arbre")
    }

    #[test]
    fn test_tree_exposes_the_window_search_and_list() {
        let harness = Harness::new();
        let update = tree_update(&harness.palette(&[]));

        assert_eq!(update.tree.as_ref().map(|tree| tree.root), Some(ROOT_ID));

        let root = node_of(&update, ROOT_ID);
        assert_eq!(root.role(), Role::Window);
        assert_eq!(root.label(), Some("Paramètres Gremlin"));
        assert_eq!(root.children(), [SEARCH_ID, LIST_ID]);

        assert_eq!(node_of(&update, SEARCH_ID).role(), Role::TextInput);
        assert_eq!(node_of(&update, LIST_ID).role(), Role::ListBox);
    }

    #[test]
    fn test_every_node_is_reachable_from_the_root() {
        // AccessKit rejette un arbre dont un nœud n'est ni la racine ni l'enfant
        // d'un autre nœud de la mise à jour.
        let harness = Harness::new();
        let mut palette = harness.palette(&[]);
        palette.enter_group(PaletteGroup::Wardrobe);
        let update = tree_update(&palette);

        let mut reachable = vec![ROOT_ID];
        let mut cursor = 0;
        while cursor < reachable.len() {
            let id = reachable[cursor];
            cursor += 1;
            if let Some((_, node)) = update.nodes.iter().find(|(node_id, _)| *node_id == id) {
                reachable.extend(node.children());
            }
        }

        for (id, _) in &update.nodes {
            assert!(
                reachable.contains(id),
                "le nœud {id:?} n'est atteignable depuis aucun parent"
            );
        }
    }

    #[test]
    fn test_focus_follows_the_selection() {
        // C'est ce qui fait annoncer chaque déplacement au clavier : sans focus
        // mobile, un lecteur d'écran reste muet sur les flèches.
        let harness = Harness::new();
        let mut palette = harness.palette(&[]);
        palette.enter_group(PaletteGroup::Wardrobe);

        assert_eq!(tree_update(&palette).focus, row_id(0));

        palette.select_next();
        assert_eq!(tree_update(&palette).focus, row_id(1));

        palette.select_next();
        assert_eq!(tree_update(&palette).focus, row_id(2));
    }

    #[test]
    fn test_focus_falls_back_to_the_search_field_when_empty() {
        let harness = Harness::new();
        let mut palette = harness.palette(&[]);
        palette.set_query("zzz-aucune-correspondance");

        assert_eq!(palette.filtered_len(), 0);
        let update = tree_update(&palette);
        assert_eq!(update.focus, SEARCH_ID);
    }

    #[test]
    fn test_toggles_are_announced_as_switches_with_their_state() {
        let harness = Harness::new();
        let mut palette = harness.palette(&[]);
        palette.set_query("click-through");

        let update = tree_update(&palette);
        let node = node_of(&update, row_id(0));
        assert_eq!(
            node.role(),
            Role::Switch,
            "une bascule doit être annoncée comme telle"
        );
        assert_eq!(
            node.toggled(),
            Some(Toggled::False),
            "l'état de la bascule doit être exposé"
        );
    }

    #[test]
    fn test_group_entries_announce_that_they_open_a_sub_list() {
        let harness = Harness::new();
        let palette = harness.palette(&[]);
        let update = tree_update(&palette);

        let description = node_of(&update, row_id(0))
            .description()
            .expect("une entrée de racine doit être décrite");
        assert!(
            description.contains("sous-liste"),
            "rien ne distingue à l'oreille une entrée de groupe : {description}"
        );
    }

    #[test]
    fn test_rows_carry_the_actions_a_screen_reader_needs() {
        let harness = Harness::new();
        let palette = harness.palette(&[]);
        let update = tree_update(&palette);
        let node = node_of(&update, row_id(0));

        for action in [Action::Click, Action::Focus, Action::ScrollIntoView] {
            assert!(
                node.supports_action(action),
                "l'action {action:?} manque sur une ligne"
            );
        }
    }

    #[test]
    fn test_search_field_exposes_its_value_and_result_count() {
        let harness = Harness::new();
        let mut palette = harness.palette(&[]);
        palette.set_query("soigner");

        let update = tree_update(&palette);
        let search = node_of(&update, SEARCH_ID);
        assert_eq!(search.value(), Some("soigner"));

        let description = search.description().expect("description du champ");
        assert!(
            description.contains("résultat"),
            "le décompte doit être annoncé : {description}"
        );
    }

    #[test]
    fn test_breadcrumb_names_the_list() {
        let harness = Harness::new();
        let mut palette = harness.palette(&[]);
        assert_eq!(
            node_of(&tree_update(&palette), LIST_ID).label(),
            Some("Commandes")
        );

        palette.enter_group(PaletteGroup::Wardrobe);
        assert_eq!(
            node_of(&tree_update(&palette), LIST_ID).label(),
            Some("Garde-robe")
        );
    }

    #[test]
    fn test_exposed_rows_are_capped_without_breaking_the_tree() {
        let harness = Harness::new();
        let repos: Vec<RepoDisplayInfo> = (0..MAX_EXPOSED_ROWS + 200)
            .map(|index| RepoDisplayInfo {
                path: test_repo_path(&format!("dépôt-{index}")),
                name: format!("dépôt-{index}"),
                branch: Some(String::from("main")),
                last_commit_msg: None,
                status: RepoTrackingStatus::Active,
                issue: None,
            })
            .collect();

        let mut palette = harness.palette(&repos);
        palette.enter_group(PaletteGroup::Repos);
        assert!(palette.filtered_len() > MAX_EXPOSED_ROWS);

        let update = tree_update(&palette);
        let list = node_of(&update, LIST_ID);
        assert_eq!(list.children().len(), MAX_EXPOSED_ROWS);

        // Le plafond ne doit pas laisser le focus pointer un nœud absent.
        assert!(
            update.nodes.iter().any(|(id, _)| *id == update.focus),
            "le focus désigne un nœud qui n'est pas dans l'arbre"
        );
    }

    #[test]
    fn test_row_identifiers_round_trip() {
        for index in [0_usize, 1, 42, MAX_EXPOSED_ROWS - 1] {
            assert_eq!(row_index(row_id(index)), Some(index));
            assert!(!is_row_action(row_id(index)));

            // Le bouton d'une ligne désigne la même ligne, mais se distingue.
            assert_eq!(row_index(row_action_id(index)), Some(index));
            assert!(is_row_action(row_action_id(index)));
            assert_ne!(row_action_id(index), row_id(index));
        }
        // Les nœuds fixes ne doivent pas être pris pour des lignes.
        assert_eq!(row_index(ROOT_ID), None);
        assert_eq!(row_index(SEARCH_ID), None);
        assert_eq!(row_index(LIST_ID), None);
    }

    #[test]
    fn test_row_action_is_exposed_as_a_named_button() {
        // Le retrait d'un dépôt ne doit pas être une commande réservée à la
        // souris : elle existe comme bouton nommé dans l'arbre.
        let harness = Harness::new();
        let repos = vec![RepoDisplayInfo {
            path: test_repo_path("alpha"),
            name: "alpha".into(),
            branch: Some("main".into()),
            last_commit_msg: None,
            status: RepoTrackingStatus::Active,
            issue: None,
        }];

        let mut palette = harness.palette(&repos);
        palette.enter_group(PaletteGroup::Repos);

        let row = (0..palette.filtered_len())
            .find(|index| {
                palette
                    .filtered_item(*index)
                    .is_some_and(|item| item.row_action.is_some())
            })
            .expect("une ligne de dépôt doit porter une action");

        let update = tree_update(&palette);
        let button = node_of(&update, row_action_id(row));

        assert_eq!(button.role(), Role::Button);
        assert_eq!(
            button.label(),
            Some("Retirer alpha de la surveillance"),
            "le bouton doit se nommer, sinon il est muet au lecteur d'écran"
        );
        assert!(button.supports_action(Action::Click));

        // Il est rattaché à sa ligne, et non orphelin dans l'arbre.
        assert!(node_of(&update, row_id(row))
            .children()
            .contains(&row_action_id(row)));
    }

    #[test]
    fn test_rows_without_action_expose_no_button() {
        let harness = Harness::new();
        let mut palette = harness.palette(&[]);
        palette.enter_group(PaletteGroup::Repos);

        let update = tree_update(&palette);
        assert!(
            !update.nodes.iter().any(|(id, _)| is_row_action(*id)),
            "aucun bouton ne doit exister sans ligne qui le porte"
        );
    }

    #[test]
    fn test_hostile_metadata_does_not_break_the_tree() {
        let harness = Harness::new();
        let repos = vec![RepoDisplayInfo {
            path: test_repo_path("dépôt-🐉-漢字-très-long"),
            name: "dépôt-🐉-漢字-très-long".into(),
            branch: Some("feature/été".into()),
            last_commit_msg: Some("fix: gère « l'unicode » — et le reste".into()),
            status: RepoTrackingStatus::Active,
            issue: None,
        }];

        let mut palette = harness.palette(&repos);
        palette.enter_group(PaletteGroup::Repos);
        let update = tree_update(&palette);

        assert!(!update.nodes.is_empty());
        assert!(update.nodes.iter().any(|(id, _)| *id == update.focus));
    }
}
