# Gremlin — Le compagnon de bureau pour développeurs

> Un familier virtuel de bureau ultra-léger et autonome en Rust natif, qui grandit et évolue au rythme de vos commits Git. **Zéro configuration requise.**

---

## Points forts

* **Zéro configuration (Out-of-the-Box) :** Détection automatique des dépôts Git existants et surveillance en temps réel (`git init`, `git clone`, `commit`) via les interruptions du système de fichiers de l'OS. Aucun hook manuel ni port réseau requis.
* **100 % natif :** Écrit en Rust pur avec rendu GPU accéléré (`winit` + `pixels`). La boucle d'événements ne fait jamais d'attente active : elle dort entre deux images (`ControlFlow::WaitUntil`) et n'est réveillée que par une image d'animation à afficher, une interaction ou un signal Git.
* **Accessible :** le panneau de paramètres expose son arbre sémantique au système (UI Automation, NSAccessibility, AT-SPI) et s'utilise donc au lecteur d'écran. Trois thèmes dont un à contraste renforcé, taille de texte réglable, mode mouvement réduit, et des rapports de contraste vérifiés par la suite de tests.
* **Cross-platform pur :** Fonctionne de manière native sur Windows, macOS et Linux sans empaquetage lourd ni Webview.
* **Forte personnalisation :** Moteur de rendu multi-calques modulaire (skins, chapeaux, accessoires, auras) extensible via de simples fichiers JSON et PNG.
* **Non intrusif :** Fenêtre transparente flottante sans bordure avec mode *click-through* pour coder sans gêne visuelle. Le panneau de paramètres occupe sa propre fenêtre : le familier reste visible et continue de s'animer pendant le réglage.
* **Résistant aux fichiers abîmés :** sauvegardes, configuration et manifests de skins sont validés au chargement. Une sauvegarde illisible est mise de côté plutôt qu'écrasée, et un manifest hostile ne peut ni figer ni faire planter l'application.

---

## Mécaniques de jeu

Gremlin possède un cycle de vie autonome régulé par une boucle d'état interne :

* **Énergie / Faim :** Alimentée par la fréquence des commits et l'activité de code. Baisse lentement avec l'inactivité.
* **Humeur / Joie :** Boostée par la régularité et les *pushes*. Chute en cas de dette technique ou d'inactivité prolongée. Les seuils comportent une hystérésis : une jauge oscillant autour d'un seuil ne provoque pas de clignotement d'humeur.
* **XP et Évolution :** Chaque commit rapporte de l'expérience, débloquant des formes évoluées (Bébé -> Adolescent -> Adulte -> Cyber-Gremlin) et de nouveaux cosmétiques. Seuls les vrais commits comptent : un `git checkout` ne rapporte pas d'XP.
* **États émotionnels :** Sprites dynamiques dédiés (*Happy*, *Coding*, *Hungry*, *Tired*, *Sick*, *Angry*, *Sleeping*, *Dead*).
* **Rattrapage hors-ligne :** au redémarrage, le temps écoulé depuis la dernière sauvegarde est simulé, dans la limite du plafond configuré.

---

## Cadencement et consommation

Il n'y a pas de fréquence d'affichage fixe : le rythme de réveil s'adapte au contexte, et une image n'est recomposée que si quelque chose a réellement changé.

| Contexte | Intervalle de réveil |
| --- | --- |
| Glisser-déposer de la fenêtre | 33 ms (~30 im/s) |
| Animation en cours | durée de l'image suivante, plafonnée à 60 im/s |
| Aucune animation en attente | 1 s |
| Panneau ouvert | resserre l'intervalle ci-dessus à 100 ms au plus |
| Panneau ouvert, mouvement réduit | aucun resserrement : réveil sur événement seul |

Le panneau **resserre** la cadence sans la remplacer : il occupe sa propre fenêtre, et le familier continue donc de s'animer à son rythme pendant le réglage. Ce resserrement n'a qu'une raison d'être — faire clignoter le curseur de saisie ; le mode mouvement réduit l'éteint et le supprime avec lui.

Les signaux Git et les modifications de skins ne subissent pas cette latence : ils réveillent la boucle immédiatement via le proxy de la boucle d'événements.

---

## Panneau de paramètres

Le panneau s'ouvre par `Espace` ou un clic droit sur le familier, et depuis la zone de notification. Il s'inspire de Raycast : une seule barre de recherche, tout au clavier, et un aperçu vivant à droite qui essaie l'accessoire survolé sur le familier en temps réel.

### Navigation à deux niveaux

La racine énumère cinq groupes — Profil, Soins et actions, Garde-robe, Dépôts surveillés, Préférences système — chacun avec son décompte. Cette structure existe parce que la liste plate devenait impraticable : le scan Git ne plafonne pas le nombre de dépôts détectés, et quelques dizaines suffisaient à noyer tout le reste.

Toute saisie bascule en **recherche globale**, quel que soit le niveau où l'on se trouve : inutile de remonter pour chercher ailleurs. La recherche replie les diacritiques (`depot` trouve « Dépôt ») et accepte les sous-séquences (`ez` trouve « Échelle de zoom »). Les résultats sont classés par pertinence, en gardant les sections contiguës.

### Raccourcis clavier

| Touche | Effet |
| --- | --- |
| `Tab` ou `Entrée` | descendre dans un groupe, ou exécuter la commande |
| `Échap` | gradué : effacer la recherche, puis remonter, puis fermer |
| `Retour arrière` sur saisie vide | remonter d'un niveau |
| `↑` `↓` | déplacer la sélection |
| `Page↑` `Page↓` | déplacer la sélection d'une page |
| `←` `→` `Début` `Fin` | déplacer le curseur de saisie |
| `Ctrl+U` | effacer la recherche |
| `Ctrl+W` | effacer le mot précédent |
| `Ctrl+S` | sauvegarder immédiatement |

À la souris : survol, clic pour activer, molette pour défiler, et glisser depuis une zone vide pour déplacer la fenêtre — elle n'a pas de barre de titre.

`Ctrl+A` n'est volontairement pas lié : sans modèle de sélection de texte, il ne pourrait que mentir sur son effet.

### Netteté sur écran à haute densité

Le tampon du panneau est alloué en pixels **physiques**, calculés depuis le facteur d'échelle du système, et présenté par un transfert un pour un. À 125 % ou 150 %, aucun rééchantillonnage n'intervient donc. La géométrie suit le facteur en continu ; la police, elle, est bitmap et ne s'agrandit que par facteurs entiers — d'où le réglage explicite de **taille du texte**, qui rend la main là où la mise à l'échelle automatique ne peut atteindre qu'un palier voisin.

### Accessibilité

* **Lecteur d'écran :** l'arbre sémantique du panneau est exposé au système via AccessKit. Le focus suit la sélection, chaque déplacement au clavier est donc annoncé ; les réglages à deux états sont annoncés comme des interrupteurs, avec leur état.
* **Thèmes :** sombre, clair, suivi du système, et contraste renforcé. Les rapports de contraste de chaque paire texte-sur-fond sont **vérifiés par la suite de tests** aux seuils WCAG 2.1 (4,5:1 pour le texte, 3:1 pour les composants) : une régression de lisibilité ne peut pas être commise.
* **Mouvement réduit :** fige le curseur de saisie, seule animation permanente du panneau.
* **Sélection et survol** sont signalés par un liseré d'accent — plein pour la sélection, de demi-largeur pour le survol — et non par une teinte de fond seule.

La pile d'accessibilité est isolée derrière la feature Cargo `a11y`, activée par défaut. `cargo build --no-default-features` produit un binaire sans elle.

### Planches de contrôle

La police et la mise en page sont dessinées à la main : elles se jugent en les regardant, pas en relisant des coordonnées. Deux exemples les rendent hors écran vers des fichiers PNG :

```bash
cargo run -p gremlin-app --example font_proof_sheet
```

```bash
cargo run -p gremlin-app --example panel_proof_sheet
```

---

## Stack technique

* **Langage :** Rust (édition 2021, version minimale 1.92)
* **Gestionnaire de fenêtres :** `winit` (transparence native OS, borderless et click-through via `set_cursor_hittest`)
* **Moteur de rendu 2D :** `pixels` / `wgpu` (framebuffer pixel-art accéléré par GPU) pour le familier
* **Présentation du panneau :** `softbuffer` (transfert mémoire sans GPU). Chaque instance de `pixels` construit son propre contexte wgpu : une seconde fenêtre en `pixels` aurait coûté un contexte graphique entier, contre l'objectif d'empreinte mémoire du projet.
* **Accessibilité :** `accesskit` et `accesskit_winit`, derrière la feature `a11y`
* **Typographie :** police bitmap dessinée à la main dans le dépôt, en couverture de niveaux de gris, sans dépendance de rendu de texte
* **Surveillance système :** `notify` (inotify, FSEvents, `ReadDirectoryChangesW`)
* **Résolution des dossiers standards :** `directories`
* **Persistance :** `serde_json`, écriture atomique (fichier temporaire + `fsync` + `rename`)

### Organisation du dépôt

| Crate | Rôle |
| --- | --- |
| `gremlin-core` | Logique métier pure : jauges, humeurs, XP, cycle de vie. Aucune dépendance à l'OS ni au rendu. |
| `gremlin-watcher` | Surveillance passive des dépôts Git et des dossiers d'assets. |
| `gremlin-render` | Composition multi-calques, sprites, manifests, animations. |
| `gremlin-system` | Fenêtre, zone de notification, autostart, chemins et stockage atomique. |
| `gremlin-app` | Orchestrateur : boucle d'événements, interface et persistance. |

---

## Système de personnalisation

Gremlin charge dynamiquement les skins situés dans le dossier de configuration utilisateur (`~/.config/gremlin/skins/` ou `%APPDATA%\Gremlin\config\skins\`).

Chaque pack de skin suit une structure modulaire par calques :

```text
skin_name/
├── manifest.json      # Métadonnées, animations et points d'ancrage
├── base/              # idle.png, coding.png, sick.png...
├── hats/              # headphones.png, horns.png...
├── glasses/           # sunglasses.png...
└── held/              # neon_keyboard.png, coffee_mug.png...
```

Les sprites sont dessinés sur une toile complète de 64×64 pixels, déjà positionnés : les ancres du manifest sont des métadonnées sémantiques et ne décalent pas les calques. Les points optionnels `head` et `effect_origin` positionnent les bulles et particules. Les dimensions et durées d'animation déclarées sont bornées au chargement, de sorte qu'un manifest tiers ne puisse ni figer le rendu ni provoquer une allocation démesurée.

---

## Démarrage rapide

### Prérequis

* [Rust et Cargo](https://rustup.rs/) 1.92 ou plus récent
* Sous Linux : `libxdo-dev`, `libayatana-appindicator3-dev`, `libgtk-3-dev`, `libasound2-dev`

### Compilation et lancement

```bash
git clone https://github.com/votre-user/gremlin.git
```

```bash
cargo run
```

```bash
cargo build --release
```

Le binaire autonome généré se trouve dans `target/release/gremlin`.

### Vérifications avant contribution

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

### Simulateur sans interface graphique

Le moteur de jeu est utilisable seul, ce qui permet d'observer des cycles de vie complets en quelques secondes :

```bash
cargo run -p gremlin-core --example headless_cli
```

---

## Feuille de route

- [x] **Phase 1** : Moteur d'état et cycle de vie autonome (*Headless Engine*)
- [x] **Phase 2** : Rendu 2D, boucle adaptative et fenêtre transparente multi-OS (*Display Pipeline*)
- [x] **Phase 3** : Surveillance passive des dépôts Git sans configuration (*Zero-Config Watcher*)
- [x] **Phase 4** : Composition multi-calques et moteur d'accessoires (*Asset Engine*)
- [x] **Phase 5** : Intégration système, zone de notification et publication release (*System UX & Release*)
- [x] **Phase 6** : Animations avancées, micro-particules et phylactères (*Dynamic FX & Layer Animation*)
- [x] **Phase 6bis** : Refonte du panneau de paramètres et accessibilité système (*Accessible Panel*)
- [ ] **Phase 7** : Surveillance des tests unitaires et outillage développeur (*Tooling & Process Watcher*)
- [ ] **Phase 8** : Interactions bureau, séries de productivité et bien-être (*Desk Companion & Productivity*)
- [ ] **Phase 9** : Écosystème de mods, packaging `.gremlin` et validation CLI (*Modding Hub*)
- [ ] **Phase 10** : Métrologie, signature multi-OS et distribution certifiée (*Release Engineering*)

Consultez [ROADMAP.MD](ROADMAP.MD) pour le détail exhaustif de chaque phase et ses livrables.

---

## Licence

Distribué sous licence MIT.
