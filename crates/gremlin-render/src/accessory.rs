//! Modèle de données, métadonnées et catalogue pour les accessoires et skins modulaires.
//!
//! Comme les manifests de skins, les manifests d'accessoires proviennent de dossiers de
//! mods utilisateur : ils sont normalisés puis validés au parsing (voir [`crate::limits`]).

use crate::error::RenderError;
use crate::layer::LayerType;
use crate::limits::{clamp_frame_duration_ms, CANVAS_SIZE, MAX_FRAME_DIMENSION};
use crate::manifest::{default_frame_duration_ms, AnchorPoint};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;
use tracing::warn;

/// Catégorie d'accessoire cosmétique.
///
/// Ajouter une catégorie se fait en trois gestes seulement : une variante ici, l'entrée
/// correspondante dans [`AccessoryCategory::ALL`], et la variante de calque associée
/// dans [`LayerType`]. Tout le reste (garde-robe, composition, palette) est piloté par
/// itération sur `ALL`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum AccessoryCategory {
    /// Chapeaux, couronnes, casquettes, casques.
    Hat,
    /// Lunettes, visières, monocles.
    Glasses,
    /// Sweats, vestes, costumes, capes.
    Outfit,
    /// Objets tenus (tasse, clavier, baguette).
    Held,
    /// Effets d'arrière-plan ou halos lumineux.
    Aura,
}

impl AccessoryCategory {
    /// Toutes les catégories, dans l'ordre d'empilement des calques (z-index croissant).
    ///
    /// C'est la liste de référence : toute itération sur les catégories doit passer
    /// par elle plutôt que d'énumérer les variantes à la main.
    pub const ALL: [Self; 5] = [
        Self::Aura,
        Self::Outfit,
        Self::Glasses,
        Self::Hat,
        Self::Held,
    ];

    /// Convertit la catégorie en type de calque d'affichage correspondant.
    #[must_use]
    pub const fn to_layer_type(self) -> LayerType {
        match self {
            Self::Aura => LayerType::Aura,
            Self::Outfit => LayerType::Outfit,
            Self::Glasses => LayerType::Glasses,
            Self::Hat => LayerType::Hat,
            Self::Held => LayerType::Held,
        }
    }

    /// Nom d'ancrage associé par défaut dans le manifest du skin.
    ///
    /// Source unique de vérité : délègue à [`LayerType::anchor_name`].
    #[must_use]
    pub const fn default_anchor_name(self) -> &'static str {
        self.to_layer_type().anchor_name()
    }

    /// Nom d'affichage pour l'interface utilisateur.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Hat => "Chapeaux & Couvre-chefs",
            Self::Glasses => "Lunettes & Visières",
            Self::Outfit => "Tenues & Costumes",
            Self::Held => "Objets Tenus",
            Self::Aura => "Auras & Effets",
        }
    }

    /// Icône représentative pour la palette de commande.
    #[must_use]
    pub const fn icon(self) -> &'static str {
        match self {
            Self::Hat => "🎩",
            Self::Glasses => "🕶️",
            Self::Outfit => "👕",
            Self::Held => "☕",
            Self::Aura => "🌌",
        }
    }
}

/// Définition et métadonnées d'un accessoire modulaire.
///
/// # Convention de dessin
/// Les frames d'un accessoire sont dessinées sur un canevas **pleine taille**
/// ([`CANVAS_SIZE`] x [`CANVAS_SIZE`]) et déjà positionnées à leur emplacement nominal
/// sur le corps du familier. Voir [`crate::layer::LayerCompositor`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessoryManifest {
    /// Identifiant unique de l'accessoire (ex: "`wizard_hat`", "`cyber_glasses`").
    pub id: String,
    /// Nom affiché dans la garde-robe.
    pub name: String,
    /// Auteur de l'accessoire.
    #[serde(default)]
    pub author: String,
    /// Version de l'accessoire.
    #[serde(default)]
    pub version: String,
    /// Catégorie de l'accessoire.
    pub category: AccessoryCategory,
    /// Description courte de l'accessoire.
    #[serde(default)]
    pub description: String,
    /// Largeur de la frame en pixels (ex: 64), bornée par [`MAX_FRAME_DIMENSION`].
    #[serde(default = "default_dimension")]
    pub frame_width: u32,
    /// Hauteur de la frame en pixels (ex: 64), bornée par [`MAX_FRAME_DIMENSION`].
    #[serde(default = "default_dimension")]
    pub frame_height: u32,
    /// Liste ordonnée des clés de frames de sprites.
    ///
    /// Les frames au-delà de la première ne sont consommées que par les points
    /// d'entrée animés du compositeur — voir [`AccessoryManifest::frame_key_at`] et
    /// [`crate::layer::LayerCompositor::compose_layered_pet_animated`]. Le rendu
    /// statique ([`crate::layer::LayerCompositor::compose_layered_pet`]) n'utilise
    /// que [`AccessoryManifest::primary_frame_key`].
    pub frames: Vec<String>,
    /// Durée d'une frame d'animation en millisecondes.
    ///
    /// Toujours ramenée dans les bornes autorisées par le parsing ; une valeur nulle
    /// empêcherait la sélection de frame de converger.
    #[serde(default = "default_frame_duration_ms")]
    pub frame_duration_ms: u64,
    /// Décalages spécifiques par état émotionnel (ex: "happy" -> offset {x, y}).
    #[serde(default)]
    pub offsets_per_mood: BTreeMap<String, AnchorPoint>,
}

const fn default_dimension() -> u32 {
    CANVAS_SIZE
}

impl AccessoryManifest {
    /// Parse un manifest d'accessoire depuis une chaîne JSON, puis le normalise et le valide.
    ///
    /// # Errors
    /// - `RenderError::InvalidManifest` si le JSON est invalide ;
    /// - `RenderError::InvalidManifestField` si une dimension est hors bornes.
    pub fn from_json(json_str: &str) -> Result<Self, RenderError> {
        let mut manifest: Self = serde_json::from_str(json_str)?;
        manifest.normalize();
        manifest.validate()?;
        Ok(manifest)
    }

    /// Ramène la durée de frame dans ses bornes autorisées.
    pub fn normalize(&mut self) {
        let (clamped, adjusted) = clamp_frame_duration_ms(self.frame_duration_ms);
        if adjusted {
            warn!(
                accessory = %self.id,
                raw = self.frame_duration_ms,
                clamped,
                "Durée de frame d'accessoire hors bornes : valeur rabotée"
            );
            self.frame_duration_ms = clamped;
        }
    }

    /// Vérifie que les dimensions déclarées respectent les bornes de sécurité.
    ///
    /// # Errors
    /// Renvoie `RenderError::InvalidManifestField` si une dimension est nulle ou démesurée.
    pub fn validate(&self) -> Result<(), RenderError> {
        for (field, value) in [
            ("frame_width", self.frame_width),
            ("frame_height", self.frame_height),
        ] {
            if value == 0 {
                return Err(RenderError::invalid_field(
                    field,
                    "doit être strictement positif",
                ));
            }
            if value > MAX_FRAME_DIMENSION {
                return Err(RenderError::invalid_field(
                    field,
                    format!("{value} px dépasse la borne de {MAX_FRAME_DIMENSION} px"),
                ));
            }
        }
        Ok(())
    }

    /// Récupère la première frame (principale / statique) de l'accessoire.
    #[must_use]
    pub fn primary_frame_key(&self) -> Option<&str> {
        self.frames.first().map(String::as_str)
    }

    /// Sélectionne la frame à afficher après `elapsed` de lecture, en boucle.
    ///
    /// Les accessoires mono-frame renvoient toujours leur unique frame.
    #[must_use]
    pub fn frame_key_at(&self, elapsed: Duration) -> Option<&str> {
        if self.frames.len() <= 1 {
            return self.primary_frame_key();
        }

        let (duration_ms, _) = clamp_frame_duration_ms(self.frame_duration_ms);
        let index = (elapsed.as_millis() / u128::from(duration_ms)) as usize % self.frames.len();
        self.frames.get(index).map(String::as_str)
    }

    /// Récupère le décalage spécifique pour une humeur donnée, ou un décalage nul par défaut.
    #[must_use]
    pub fn mood_offset(&self, mood_key: &str) -> (i32, i32) {
        self.offsets_per_mood
            .get(mood_key)
            .map_or((0, 0), |pt| (pt.x, pt.y))
    }
}

/// Description d'un accessoire enregistré dans le catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessoryItem {
    /// Métadonnées de l'accessoire.
    pub manifest: AccessoryManifest,
    /// Indique si l'accessoire est généré de manière procédurale (intégré) ou chargé depuis un mod.
    pub is_procedural: bool,
    /// Chemin du dossier source sur le disque si applicable.
    pub source_path: Option<std::path::PathBuf>,
}

impl AccessoryItem {
    /// Crée un nouvel accessoire procédural intégré.
    #[must_use]
    pub const fn procedural(manifest: AccessoryManifest) -> Self {
        Self {
            manifest,
            is_procedural: true,
            source_path: None,
        }
    }

    /// Crée un nouvel accessoire chargé depuis un répertoire de mod utilisateur.
    #[must_use]
    pub fn from_mod<P: AsRef<Path>>(manifest: AccessoryManifest, path: P) -> Self {
        Self {
            manifest,
            is_procedural: false,
            source_path: Some(path.as_ref().to_path_buf()),
        }
    }

    /// Identifiant unique de l'accessoire.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    /// Catégorie de l'accessoire.
    #[must_use]
    pub const fn category(&self) -> AccessoryCategory {
        self.manifest.category
    }
}

/// Catalogue centralisé des accessoires disponibles.
#[derive(Debug, Default, Clone)]
pub struct AccessoryCatalog {
    items: BTreeMap<String, AccessoryItem>,
}

impl AccessoryCatalog {
    /// Crée un nouveau catalogue vide.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Nombre d'accessoires enregistrés dans le catalogue.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Indique si le catalogue est vide.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Enregistre un accessoire dans le catalogue.
    pub fn register(&mut self, item: AccessoryItem) {
        self.items.insert(item.id().to_string(), item);
    }

    /// Supprime un accessoire par son ID.
    pub fn unregister(&mut self, id: &str) -> Option<AccessoryItem> {
        self.items.remove(id)
    }

    /// Recherche un accessoire par son identifiant unique.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&AccessoryItem> {
        self.items.get(id)
    }

    /// Liste tous les accessoires triés par nom.
    #[must_use]
    pub fn all_items(&self) -> Vec<&AccessoryItem> {
        let mut list: Vec<&AccessoryItem> = self.items.values().collect();
        list.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
        list
    }

    /// Liste les accessoires appartenant à une catégorie spécifique.
    #[must_use]
    pub fn items_by_category(&self, category: AccessoryCategory) -> Vec<&AccessoryItem> {
        let mut list: Vec<&AccessoryItem> = self
            .items
            .values()
            .filter(|item| item.category() == category)
            .collect();
        list.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
        list
    }

    /// Filtre les accessoires par recherche textuelle insensible à la casse (nom, description, ID).
    #[must_use]
    pub fn search(&self, query: &str) -> Vec<&AccessoryItem> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.all_items();
        }

        let mut matched: Vec<&AccessoryItem> = self
            .items
            .values()
            .filter(|item| {
                item.manifest.name.to_lowercase().contains(&q)
                    || item.manifest.id.to_lowercase().contains(&q)
                    || item.manifest.description.to_lowercase().contains(&q)
                    || item.manifest.author.to_lowercase().contains(&q)
            })
            .collect();

        matched.sort_by(|a, b| a.manifest.name.cmp(&b.manifest.name));
        matched
    }

    /// Supprime tous les accessoires non procéduraux (utile avant un scan de hot-reload).
    pub fn clear_mods(&mut self) {
        self.items.retain(|_, item| item.is_procedural);
    }
}

/// État d'équipement de la garde-robe du Gremlin.
///
/// Les emplacements sont stockés dans une table indexée par catégorie : ajouter une
/// catégorie d'accessoire ne demande aucune modification ici.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WardrobeEquipment {
    /// Skin de base actif.
    #[serde(default = "default_skin_id")]
    pub skin_id: String,
    /// Accessoire équipé par catégorie ; une catégorie absente signifie « rien d'équipé ».
    #[serde(default)]
    pub slots: BTreeMap<AccessoryCategory, String>,
}

fn default_skin_id() -> String {
    String::from("default")
}

impl Default for WardrobeEquipment {
    fn default() -> Self {
        Self {
            skin_id: default_skin_id(),
            slots: BTreeMap::new(),
        }
    }
}

impl WardrobeEquipment {
    /// Crée un équipement avec les valeurs par défaut.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Récupère l'accessoire équipé pour une catégorie donnée.
    #[must_use]
    pub fn get_equipped(&self, category: AccessoryCategory) -> Option<&str> {
        self.slots.get(&category).map(String::as_str)
    }

    /// Équipe un accessoire dans le slot correspondant à sa catégorie.
    pub fn equip(&mut self, category: AccessoryCategory, accessory_id: impl Into<String>) {
        self.slots.insert(category, accessory_id.into());
    }

    /// Retire l'accessoire équipé dans une catégorie donnée.
    pub fn unequip(&mut self, category: AccessoryCategory) {
        self.slots.remove(&category);
    }

    /// Bascule l'équipement d'un accessoire : l'équipe s'il n'est pas actif dans
    /// **cette catégorie**, le retire s'il l'est déjà.
    ///
    /// La bascule est volontairement limitée à la catégorie passée : tester toutes les
    /// catégories ferait échouer silencieusement `toggle(Glasses, "x")` lorsque `"x"`
    /// est équipé dans un autre emplacement.
    pub fn toggle(&mut self, category: AccessoryCategory, accessory_id: &str) {
        if self.get_equipped(category) == Some(accessory_id) {
            self.unequip(category);
        } else {
            self.equip(category, accessory_id);
        }
    }

    /// Vérifie si un accessoire spécifique est équipé, quelle que soit la catégorie.
    #[must_use]
    pub fn is_equipped(&self, accessory_id: &str) -> bool {
        self.slots.values().any(|id| id == accessory_id)
    }

    /// Vérifie si un accessoire est équipé dans une catégorie précise.
    #[must_use]
    pub fn is_equipped_in(&self, category: AccessoryCategory, accessory_id: &str) -> bool {
        self.get_equipped(category) == Some(accessory_id)
    }

    /// Itère sur les emplacements occupés, dans l'ordre d'empilement des calques.
    pub fn equipped_slots(&self) -> impl Iterator<Item = (AccessoryCategory, &str)> + '_ {
        AccessoryCategory::ALL
            .into_iter()
            .filter_map(|category| Some((category, self.get_equipped(category)?)))
    }

    /// Retire tous les accessoires équipés tout en conservant le skin de base.
    pub fn clear_all_accessories(&mut self) {
        self.slots.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::MIN_FRAME_DURATION_MS;

    /// Construit un manifest d'accessoire minimal pour les tests.
    fn manifest(id: &str, name: &str, category: AccessoryCategory) -> AccessoryManifest {
        AccessoryManifest {
            id: id.into(),
            name: name.into(),
            author: "Dev".into(),
            version: "1.0".into(),
            category,
            description: String::new(),
            frame_width: CANVAS_SIZE,
            frame_height: CANVAS_SIZE,
            frames: vec![id.into()],
            frame_duration_ms: 200,
            offsets_per_mood: BTreeMap::new(),
        }
    }

    #[test]
    fn test_accessory_manifest_parsing() {
        let json = r#"{
            "id": "wizard_hat",
            "name": "Chapeau de Mage",
            "author": "Gremlin Studio",
            "version": "1.0.0",
            "category": "Hat",
            "description": "Un chapeau magique bleu nuit étoilé.",
            "frame_width": 64,
            "frame_height": 64,
            "frames": ["wizard_hat_0", "wizard_hat_1"],
            "frame_duration_ms": 250,
            "offsets_per_mood": {
                "happy": { "x": 0, "y": -2 },
                "sleep": { "x": 1, "y": 2 }
            }
        }"#;

        let manifest = match AccessoryManifest::from_json(json) {
            Ok(m) => m,
            Err(e) => panic!("Échec de parsing du manifest d'accessoire : {e}"),
        };

        assert_eq!(manifest.id, "wizard_hat");
        assert_eq!(manifest.category, AccessoryCategory::Hat);
        assert_eq!(manifest.frames.len(), 2);
        assert_eq!(manifest.mood_offset("happy"), (0, -2));
        assert_eq!(manifest.mood_offset("hungry"), (0, 0));
        assert_eq!(manifest.primary_frame_key(), Some("wizard_hat_0"));
    }

    #[test]
    fn test_accessory_manifest_malforme_est_rejete() {
        assert!(matches!(
            AccessoryManifest::from_json("{"),
            Err(RenderError::InvalidManifest(_))
        ));

        // Catégorie inconnue.
        let json = r#"{ "id": "a", "name": "A", "category": "Cape", "frames": ["a"] }"#;
        assert!(matches!(
            AccessoryManifest::from_json(json),
            Err(RenderError::InvalidManifest(_))
        ));

        // Dimension démesurée.
        let json = r#"{
            "id": "a", "name": "A", "category": "Hat", "frames": ["a"],
            "frame_width": 4000000000, "frame_height": 64
        }"#;
        assert!(matches!(
            AccessoryManifest::from_json(json),
            Err(RenderError::InvalidManifestField { .. })
        ));
    }

    #[test]
    fn test_accessory_manifest_duree_nulle_est_rabotee() {
        let json = r#"{
            "id": "a", "name": "A", "category": "Hat",
            "frames": ["a0", "a1"], "frame_duration_ms": 0
        }"#;
        let manifest = match AccessoryManifest::from_json(json) {
            Ok(m) => m,
            Err(e) => panic!("parsing inattendu : {e}"),
        };
        // Une durée nulle rendrait la sélection de frame non calculable (division par zéro).
        assert_eq!(manifest.frame_duration_ms, MIN_FRAME_DURATION_MS);
        assert_eq!(manifest.frame_key_at(Duration::ZERO), Some("a0"));
        assert_eq!(manifest.frame_key_at(Duration::from_millis(1)), Some("a1"));
    }

    #[test]
    fn test_frame_key_at_boucle_sur_les_frames() {
        let mut m = manifest("anim", "Anim", AccessoryCategory::Aura);
        m.frames = vec!["f0".into(), "f1".into(), "f2".into()];
        m.frame_duration_ms = 100;

        assert_eq!(m.frame_key_at(Duration::ZERO), Some("f0"));
        assert_eq!(m.frame_key_at(Duration::from_millis(150)), Some("f1"));
        assert_eq!(m.frame_key_at(Duration::from_millis(250)), Some("f2"));
        assert_eq!(m.frame_key_at(Duration::from_millis(350)), Some("f0"));
    }

    #[test]
    fn test_frame_key_at_mono_frame_est_stable() {
        let m = manifest("static", "Statique", AccessoryCategory::Hat);
        assert_eq!(m.frame_key_at(Duration::from_secs(9999)), Some("static"));
    }

    #[test]
    fn test_categories_couvrent_tous_les_calques() {
        assert_eq!(AccessoryCategory::ALL.len(), LayerType::ALL.len() - 1);
        for category in AccessoryCategory::ALL {
            assert_eq!(
                category.default_anchor_name(),
                category.to_layer_type().anchor_name(),
                "le nom d'ancrage doit avoir une source unique de vérité"
            );
        }
    }

    #[test]
    fn test_accessory_catalog_and_search() {
        let mut catalog = AccessoryCatalog::new();

        let mut hat = manifest("wizard_hat", "Chapeau de Mage", AccessoryCategory::Hat);
        hat.description = "Chapeau bleu".into();
        let mut glasses = manifest(
            "vr_visor",
            "Visière VR Cyberpunk",
            AccessoryCategory::Glasses,
        );
        glasses.description = "Réalité augmentée".into();

        catalog.register(AccessoryItem::procedural(hat));
        catalog.register(AccessoryItem::procedural(glasses));

        assert_eq!(catalog.len(), 2);
        assert_eq!(catalog.items_by_category(AccessoryCategory::Hat).len(), 1);
        assert_eq!(catalog.items_by_category(AccessoryCategory::Aura).len(), 0);

        let search_res = catalog.search("Cyberpunk");
        assert_eq!(search_res.len(), 1);
        assert_eq!(search_res[0].id(), "vr_visor");
    }

    #[test]
    fn test_wardrobe_equipment_toggle_and_slots() {
        let mut wardrobe = WardrobeEquipment::new();
        assert_eq!(wardrobe.skin_id, "default");
        assert!(wardrobe.get_equipped(AccessoryCategory::Hat).is_none());

        wardrobe.equip(AccessoryCategory::Hat, "wizard_hat");
        assert!(wardrobe.is_equipped("wizard_hat"));
        assert_eq!(
            wardrobe.get_equipped(AccessoryCategory::Hat),
            Some("wizard_hat")
        );

        // Toggle pour déséquiper
        wardrobe.toggle(AccessoryCategory::Hat, "wizard_hat");
        assert!(!wardrobe.is_equipped("wizard_hat"));
        assert!(wardrobe.get_equipped(AccessoryCategory::Hat).is_none());

        // Toggle pour rééquiper
        wardrobe.toggle(AccessoryCategory::Hat, "wizard_hat");
        assert!(wardrobe.is_equipped("wizard_hat"));

        // Remplacement d'équipement dans le même slot
        wardrobe.equip(AccessoryCategory::Hat, "crown");
        assert_eq!(wardrobe.get_equipped(AccessoryCategory::Hat), Some("crown"));
        assert!(!wardrobe.is_equipped("wizard_hat"));
    }

    #[test]
    fn test_toggle_est_limite_a_la_categorie() {
        // Régression : `toggle` consultait tous les emplacements, si bien qu'équiper
        // un identifiant dans une catégorie rendait sa bascule inopérante ailleurs.
        let mut wardrobe = WardrobeEquipment::new();
        wardrobe.equip(AccessoryCategory::Hat, "shared_id");

        wardrobe.toggle(AccessoryCategory::Glasses, "shared_id");
        assert_eq!(
            wardrobe.get_equipped(AccessoryCategory::Glasses),
            Some("shared_id"),
            "la bascule doit équiper la catégorie visée"
        );
        assert_eq!(
            wardrobe.get_equipped(AccessoryCategory::Hat),
            Some("shared_id"),
            "l'autre catégorie ne doit pas être touchée"
        );

        wardrobe.toggle(AccessoryCategory::Glasses, "shared_id");
        assert!(wardrobe.get_equipped(AccessoryCategory::Glasses).is_none());
        assert!(wardrobe.is_equipped_in(AccessoryCategory::Hat, "shared_id"));
    }

    #[test]
    fn test_equipped_slots_suit_l_ordre_des_calques() {
        let mut wardrobe = WardrobeEquipment::new();
        wardrobe.equip(AccessoryCategory::Held, "mug");
        wardrobe.equip(AccessoryCategory::Aura, "fire");
        wardrobe.equip(AccessoryCategory::Hat, "crown");

        let ordered: Vec<AccessoryCategory> = wardrobe.equipped_slots().map(|(c, _)| c).collect();
        assert_eq!(
            ordered,
            vec![
                AccessoryCategory::Aura,
                AccessoryCategory::Hat,
                AccessoryCategory::Held
            ]
        );

        wardrobe.clear_all_accessories();
        assert_eq!(wardrobe.equipped_slots().count(), 0);
        assert_eq!(wardrobe.skin_id, "default");
    }

    #[test]
    fn test_wardrobe_serde_roundtrip() {
        let mut wardrobe = WardrobeEquipment::new();
        wardrobe.equip(AccessoryCategory::Hat, "wizard_hat");
        wardrobe.equip(AccessoryCategory::Aura, "fire_aura");

        let json = match serde_json::to_string(&wardrobe) {
            Ok(j) => j,
            Err(e) => panic!("sérialisation impossible : {e}"),
        };
        let decoded: WardrobeEquipment = match serde_json::from_str(&json) {
            Ok(w) => w,
            Err(e) => panic!("désérialisation impossible : {e}"),
        };
        assert_eq!(decoded, wardrobe);
    }

    #[test]
    fn test_wardrobe_tolere_une_config_partielle() {
        let decoded: WardrobeEquipment = match serde_json::from_str("{}") {
            Ok(w) => w,
            Err(e) => panic!("désérialisation impossible : {e}"),
        };
        assert_eq!(decoded, WardrobeEquipment::default());
    }
}
