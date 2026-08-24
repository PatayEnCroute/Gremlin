# Directives de code & Bonnes pratiques Rust — Gremlin

Ce document définit les standards d'architecture, de performance et de qualité de code pour le développement du projet **Gremlin** en Rust natif.

---

## 1. Architecture & Découplage des domaines

* **Séparation stricte (Clean Architecture / Data-Driven).** Le workspace comporte cinq caisses, dont les dépendances forment un graphe acyclique :
  * `gremlin-core` : logique pure du jeu (état, décroissance, XP, transitions d'humeur). Aucune dépendance à l'OS, au rendu ou au temps réel — le temps écoulé est **injecté** via `tick(Duration)`, jamais lu depuis une horloge.
  * `gremlin-render` : rendu 2D, chargement des sprites, composition multi-calques.
  * `gremlin-system` : spécificités OS (fenêtre borderless, transparence, systray, autostart, chemins XDG/AppData, écriture atomique).
  * `gremlin-watcher` : surveillance passive du système de fichiers (`notify`) et lecture des métadonnées Git.
  * `gremlin-app` : orchestration. La logique vit dans la **bibliothèque** `gremlin_app` ; `main.rs` n'est qu'un point d'entrée mince, ce qui rend l'orchestrateur testable.
* **Aucune dépendance croisée entre les quatre caisses de domaine.** Elles ne se connaissent pas : seul `gremlin-app` les assemble.
* **Communication par passage de messages :**
  * Privilégier `crossbeam-channel` plutôt que de propager des `Arc<Mutex<T>>` à travers l'application.
  * Les canaux entre threads sont **bornés**. Un canal non borné alimenté par des événements de système de fichiers laisse la mémoire croître sans limite sous charge.
  * Les threads de surveillance émettent des signaux (`DevSignal`, `AssetSignal`), la boucle principale les consomme.
* **Pas de God-object.** Si une structure d'orchestration dépasse une douzaine de champs, regrouper les champs corrélés dans des sous-structures dédiées (`UiState`, `LoopClocks`, `Visuals`, `WatcherBridge`).

---

## 2. Performance, CPU & Gestion des ressources

Puisque l'application tourne en permanence en arrière-plan :

* **Zéro spin-lock / attente active :**
  * Configurer la boucle `winit` en `ControlFlow::WaitUntil` ou `ControlFlow::Wait`, jamais en `ControlFlow::Poll`.
  * Ne redessiner que si une image d'animation a changé ou si un événement a modifié l'état.
  * Un thread au repos doit **bloquer** sur `recv`, jamais boucler sur un `try_recv` suivi d'un `sleep`.
* **Debouncing des événements de fichiers :** un simple `git commit` déclenche des dizaines d'événements en quelques millisecondes ; les consolider (100 à 250 ms).
* **Surveiller étroit plutôt que large.** Une surveillance récursive d'une racine entière consomme un descripteur inotify par répertoire et inonde le canal à chaque build. Appliquer les exclusions (`node_modules`, `target`, …) au niveau de l'enregistrement.
* **Gestion mémoire des textures :** charger les spritesheets une seule fois au démarrage ou au changement de skin, via `SpriteAtlas`. Éviter les allocations dans le corps de la boucle de rendu.

---

## 3. Gestion des erreurs & Robustesse

* **Tolérance aux pannes absolue (démon d'arrière-plan) :** l'application ne doit jamais planter sur un échec de lecture, un fichier verrouillé ou une image corrompue.
* **Interdiction des panics en runtime :**
  * Bannir `unwrap()` et `expect()` hors des tests. Les lints du workspace les signalent déjà ; ne pas les contourner.
  * **Jamais d'indexation de chaîne par octets.** `&s[0..21]` panique dès qu'une coupure tombe au milieu d'un caractère accentué. Utiliser `crate::ui::text::truncate_with_ellipsis` ou raisonner en `chars()`.
  * Se méfier aussi de l'indexation de slice, de l'arithmétique entière qui déborde en release, et des boucles dont la condition de sortie dépend d'un flottant.
  * Utiliser `thiserror` pour les erreurs internes de chaque module.
* **Jamais de faux succès.** Une fonction non implémentée sur une plateforme renvoie une erreur explicite ; elle ne journalise pas puis ne renvoie `Ok(())`. L'interface afficherait une fonctionnalité comme active alors qu'elle ne fait rien.
* **Ne pas avaler les erreurs.** `let _ = fallible()` est interdit sur un chemin que l'utilisateur a déclenché ou dont dépend l'intégrité des données. Au minimum journaliser, et remonter à l'interface ce qui la concerne (échec de sauvegarde, surveillance non enregistrée).

---

## 4. Concurrence & Thread Safety

* **États partagés minimaux :** isoler l'état mutable dans le thread principal de la boucle d'événements.
* **Pas de locks bloquants dans le thread UI.** Les I/O disque restent confinées dans un thread dédié.
* **Arrêt déterministe.** Tout thread lancé doit avoir un chemin de sortie et être joint dans `Drop`. Une boucle de travail doit servir ses commandes de contrôle même sous flot d'événements soutenu, sinon un arrêt peut bloquer.
* **Pas de `std::process::exit` dans la logique applicative.** Il court-circuite les destructeurs — threads de surveillance, surface GPU, sauvegarde finale. Router la demande de sortie vers la boucle d'événements.

---

## 5. Abstraction Multi-OS (`#[cfg]`)

* **Isolation par modules d'OS :** ne pas disséminer de `#[cfg(target_os = "...")]` au milieu de la logique métier ; encapsuler derrière un module dédié (`gremlin-system/src/platform/`, `gremlin-app/src/desktop.rs`).
* **Préférer l'abstraction existante à la réimplémentation.** Avant d'écrire du FFI, vérifier que `winit` ne fournit pas déjà l'API : le click-through est couvert par `Window::set_cursor_hittest` sur les trois systèmes.
* **Attention aux séparateurs de chemin.** Comparer des composants `Path`, jamais des sous-chaînes contenant `/` : sous Windows, `notify` livre `refs\heads\main`.
* **Respect des conventions de fichiers :** utiliser `directories` pour résoudre configuration, cache et sauvegarde.

---

## 6. Données externes : la frontière de confiance

Tout ce qui provient du disque — sauvegardes, configuration, `manifest.json` de skins, fichiers `.git` — est une **entrée non fiable**, même si elle a été écrite par l'application elle-même : un fichier peut être édité à la main, tronqué par un arrêt brutal ou fourni par un tiers.

* Chaque structure désérialisée expose une méthode de normalisation appelée immédiatement après le chargement, et cette normalisation est **idempotente**.
* Toute valeur numérique lue depuis un fichier est bornée avant usage : dimensions d'image, durées d'animation, facteurs d'échelle, intervalles, pas de simulation.
* Les flottants sont vérifiés finis. `f32::clamp` propage `NaN` : le neutraliser explicitement.
* Les structures persistées portent un numéro de version et utilisent `#[serde(default)]` au niveau du conteneur, pour rester lisibles après ajout d'un champ.
* **Ne jamais écraser une donnée qu'on n'a pas réussi à lire.** Une erreur d'entrée/sortie et une absence de fichier sont deux situations distinctes ; un fichier corrompu est mis de côté avant tout redémarrage à neuf.
* Une chaîne issue d'un fichier et injectée dans un format structuré (XML d'un plist, `Exec=` d'un `.desktop`, chemin de fichier) doit être échappée pour ce format.

---

## 7. Linter, Formatage & Qualité de code

### Règles Clippy

Les lints sont déclarés **une seule fois**, dans `[workspace.lints]` du `Cargo.toml` racine, et hérités par chaque caisse via `[lints] workspace = true`. Ne pas ajouter d'attributs `#![warn(...)]` en tête de fichier : ils dupliqueraient une configuration qui dériverait ensuite.

Sont activés : `clippy::all`, `pedantic`, `nursery`, `unwrap_used`, `expect_used`, et `unsafe_code` en avertissement. Sont désactivés volontairement : `module_name_repetitions`, `struct_excessive_bools`, `missing_const_for_fn`, et les trois lints de conversion numérique (`cast_possible_truncation`, `cast_possible_wrap`, `cast_sign_loss`) — omniprésents dans le code de rendu pixel. **Cette dernière dérogation impose de vérifier manuellement les conversions** : le compilateur ne les signalera pas.

Tout bloc `unsafe` porte un `#[allow(unsafe_code)]` local et un commentaire `SAFETY:` justifiant l'invariant tenu.

### Tests

* Un test ne modifie jamais l'arbre de travail. Écrire dans un répertoire temporaire **unique** (identifiant de processus + compteur atomique), nettoyé par un garde RAII.
* Ne pas construire un objet qui déclenche des effets système réels (scan du répertoire personnel, icône de notification) : prévoir un point d'injection, comme `AppOptions::headless`.
* Les tests d'intégration attendent un **signal précis** avec un délai généreux, jamais une durée fixe pendant laquelle ils espèrent que quelque chose arrive.
* Couvrir explicitement les entrées hostiles : durée nulle, durée absurde, `NaN`, valeurs hors bornes, JSON malformé, chaînes accentuées et multi-octets.

### Features Cargo

La pile d'accessibilité — UI Automation sous Windows, NSAccessibility sous macOS,
AT-SPI par D-Bus sous Linux — est isolée derrière la feature `a11y` de
`gremlin-app`, activée par défaut. **Les deux configurations doivent rester
compilables et sans avertissement** : une feature dont un seul chemin est
vérifié se dégrade sans que personne ne le voie. La CI passe donc Clippy deux
fois.

### Interface : ce qui se juge en regardant

La police du panneau et sa mise en page sont dessinées à la main. Une retouche ne
se valide pas à la lecture du code : il faut regarder le rendu. Deux exemples le
produisent hors écran, sous `target/` :

```bash
cargo run -p gremlin-app --example font_proof_sheet
```

```bash
cargo run -p gremlin-app --example panel_proof_sheet
```

Sur la planche de police, les traits rouges verticaux marquent la largeur
renvoyée par `font::measure` : ils doivent affleurer la fin du texte. Un écart
signale que la mesure et le rendu ont divergé, ce qui décale le curseur de saisie.

### Contraste : vérifié, jamais supposé

Toute couleur de `ui::theme` est soumise aux tests de contraste WCAG 2.1 —
4,5:1 pour le texte, 3:1 pour les composants porteurs d'état, seuil de
perceptibilité pour le décoratif. **Ne pas contourner un échec en abaissant le
seuil.** Deux issues légitimes existent : corriger la couleur, ou démontrer que
l'information est portée par un autre élément — et alors tester *cet* élément.
C'est ce qui a été fait pour la sélection et le survol, signalés par un liseré
d'accent plutôt que par une teinte de fond.

### Commandes de vérification systématiques avant tout commit

```bash
cargo fmt --all -- --check
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo clippy -p gremlin-app --no-default-features --all-targets -- -D warnings
```

```bash
cargo test --workspace
```

```bash
cargo deny check
```

---

## 8. Structure de nommage & Conventions Rust

* **Types & Traits :** `UpperCamelCase` (`PetState`, `SpriteLayer`, `EventDispatcher`).
* **Fonctions & Variables :** `snake_case` (`calculate_decay`, `last_tick_at`).
* **Constantes :** `SCREAMING_SNAKE_CASE` (`MAX_ENERGY`, `DECAY_RATE_PER_MINUTE`). Toute valeur numérique qui apparaît deux fois, ou dont la signification n'est pas évidente à la lecture, devient une constante nommée.
* **Constructeurs :** `pub fn new(...) -> Self` ou `pub fn with_capacity(...) -> Self`. Si un type dérive `Default`, vérifier que `Default::default()` et `new()` produisent le **même** résultat.
* **Conversions :** implémenter `From` / `Into` ou `TryFrom` / `TryInto` plutôt que des méthodes personnalisées.
* **Encapsulation :** un agrégat porteur d'invariants (bornes de jauges, cohérence entre champs dérivés) garde ses champs privés et expose des accesseurs. Des champs `pub` rendent l'invariant indéfendable.
* **Langue :** commentaires et documentation en français. Les libellés destinés à l'affichage sont centralisés dans une fonction unique par domaine, jamais dispersés dans la logique.
