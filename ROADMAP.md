# Feuille de route — Gremlin (Rust Natif)

Ce document décrit les étapes d'implémentation incrémentales pour développer **Gremlin**, du moteur autonome jusqu'aux binaires finaux distribuables.

Les phases 1 à 7 sont livrées, ainsi que la phase 6bis consacrée au panneau de paramètres et à son accessibilité. Les phases 8 à 10 constituent la feuille de route active pour étendre les interactions de bureau, la personnalisation et la distribution certifiée.

---

## Phase 1 : Le cœur de jeu autonome (*Headless Engine*) — livrée

**Objectif :** Valider la logique métier, la décroissance des jauges, les humeurs et les gains d'XP sans aucune dépendance graphique ou OS.

* **Tâches :**
  * [x] Initialiser le workspace Cargo et configurer Clippy avec les règles strictes.
  * [x] Coder le module `gremlin_core::state` : structure `PetStats`, énumération `PetMood` et calcul du delta-time.
  * [x] Implémenter la machine à états pour les transitions d'humeur (`HAPPY`, `HUNGRY`, `SICK`, `DEAD`), avec hystérésis pour éviter le clignotement autour des seuils.
  * [x] Développer la progression d'XP et les paliers d'évolution (Bébé -> Adolescent -> Adulte -> Cyber-Gremlin).
  * [x] Écrire la suite de tests unitaires simulant des cycles de vie complets en quelques millisecondes.
  * [x] Verrouiller la frontière de confiance : validation des montants d'action, normalisation des sauvegardes désérialisées, versionnage du format.
* **Livrable :** un module `gremlin-core` sans dépendance système, exerçable en CLI via `cargo run -p gremlin-core --example headless_cli`.

---

## Phase 2 : Rendu 2D & Fenêtre transparente (*Display Pipeline*) — livrée

**Objectif :** Ouvrir une fenêtre transparente sans bordure et afficher le premier sprite animé de Gremlin.

* **Tâches :**
  * [x] Configurer `winit` pour créer une fenêtre sans bordure, toujours visible, avec fond transparent.
  * [x] Mettre en place `pixels` / `wgpu` pour gérer le tampon mémoire de pixels.
  * [x] Créer le décodeur de textures (chargement des PNG et découpage), avec bornage explicite des dimensions décodées.
  * [x] Implémenter la boucle de rendu adaptative.
  * [x] Gérer le glisser-déposer de la fenêtre à la souris.
* **Livrable :** un exécutable affichant Gremlin sur le bureau, déplaçable au curseur avec ses animations de base.

> **Écart assumé :** l'intention initiale mentionnait « 24 FPS en action, 1 FPS en veille ». L'implémentation ne fixe aucune cadence : elle programme le prochain réveil selon le contexte (33 ms en glisser-déposer, 100 ms palette ouverte, sinon la durée de l'image d'animation suivante plafonnée à 60 im/s, et 1 s si aucune animation n'attend). Le tableau à jour figure dans le README.

---

## Phase 3 : Surveillance passive des dépôts (*Zero-Config Watcher*) — livrée

**Objectif :** Rendre Gremlin réactif aux actions Git locales sans aucune configuration requise de l'utilisateur.

* **Tâches :**
  * [x] Coder le module `scanner` avec `directories` et `walkdir` pour identifier les dépôts existants, avec profondeur bornée et exclusion des arborescences lourdes.
  * [x] Intégrer `notify` pour surveiller `.git/logs/HEAD` et `.git/refs/heads/` en arrière-plan.
  * [x] Implémenter le *debouncing* pour fusionner les rafales d'écritures de fichiers.
  * [x] Connecter les événements détectés au moteur de jeu via des canaux bornés.
  * [x] Ajouter la détection à chaud des nouveaux dépôts (`git init`, `git clone`) et de leur disparition.
  * [x] Distinguer un vrai commit d'un `checkout` via l'action du reflog, pour ne pas attribuer d'XP à un simple changement de branche.
  * [x] Remonter les incidents de surveillance (enregistrement refusé, événements perdus) à l'application au lieu de les laisser dans les journaux.
* **Livrable :** Gremlin réagit en direct dès qu'un commit est réalisé sur la machine.

---

## Phase 4 : Moteur de personnalisation (*Asset Engine*) — livrée

**Objectif :** Permettre l'empilement modulaire des accessoires via de simples fichiers JSON et PNG.

* **Tâches :**
  * [x] Définir le parseur de `manifest.json` et sa validation (dimensions, durées, ancrages).
  * [x] Coder le pipeline de composition de calques : `Aura -> Corps -> Tenue -> Lunettes -> Chapeau -> Objet`.
  * [x] Créer l'observateur du dossier utilisateur pour recharger les mods à chaud sans redémarrage.
* **Livrable :** Gremlin peut équiper des accessoires superposés sur n'importe lequel de ses états émotionnels.

> **Convention arrêtée :** les sprites sont fournis pré-positionnés sur une toile de 64×64. Les ancres du manifest décrivent la morphologie et les origines d'effets mais ne translatent aucun calque ; seuls les décalages par humeur déplacent les accessoires.

---

## Phase 5 : Intégration système & Finitions (*System UX & Release*) — livrée

**Objectif :** Rendre l'application discrète, autonome et prête pour la distribution.

* **Tâches :**
  * [x] Intégrer `tray-icon` pour le menu contextuel dans la barre des tâches (garde-robe, mode pause, quitter).
  * [x] Activer le mode *click-through* sur les trois systèmes via `winit::Window::set_cursor_hittest`.
  * [x] Mettre en place la persistance automatique de l'état à la fermeture et à intervalle régulier, en écriture atomique.
  * [x] Configurer le pipeline CI GitHub Actions (formatage, Clippy sur les trois OS, tests, `cargo-deny`, documentation).
  * [x] Publier des binaires natifs Windows, macOS (Intel et Apple Silicon) et Linux, chacun testé sur sa plateforme cible et accompagné de sa somme de contrôle.
* **Livrable :** binaires release autonomes, sans attente active ni redessin inutile.

---

## Phase 6 : Animations avancées & Effets visuels (*Dynamic FX & Layer Animation*) — livrée

**Objectif :** Enrichir le rendu visuel avec l'animation complète des accessoires superposés, un moteur de micro-particules 2D et des phylactères pixel-art non intrusifs.

* **Tâches :**
  * [x] Activer l'animation synchronisée des accessoires dans le pipeline de composition (`compose_layered_pet_animated`).
  * [x] Implémenter un système de micro-particules pixel-art ultra-léger (étincelles de commit, confettis de level-up, Zzz de sommeil, gouttes de sueur).
  * [x] Créer un moteur de phylactères / bulles de dialogue pixel-art contextuelles (encouragements, alertes d'humeur, astuces) sans rupture du mode *click-through*.
  * [x] Gérer l'interpolation et l'adoucissement des transitions entre états émotionnels pour éviter les coupures brutes de sprites.
  * [x] Maintenir la contrainte zéro-allocation dans la boucle de rendu et la réutilisation stricte des tampons graphiques.
* **Livrable :** rendu multi-calques dynamique avec accessoires animés, retours visuels particulaires et bulles d'expression contextuelles.

---

## Phase 6bis : Panneau de paramètres accessible (*Accessible Panel*) — livrée

**Objectif :** rendre le panneau de paramètres lisible, pilotable à la souris comme au clavier, navigable malgré un nombre de dépôts non plafonné, et utilisable au lecteur d'écran.

**Point de départ.** Le panneau était fonctionnel mais brut, pour cinq raisons distinctes : une police bitmap 5×7 non lissée dont les majuscules accentuées étaient dégradées (`É` → `E`) ; un tampon de taille fixe déclaré en unités logiques, donc réétiré d'un facteur non entier sur tout écran à 125 % ou 150 % ; aucune gestion de la souris, la fenêtre étant de surcroît immobilisable et jamais centrée ; une liste plate de vingt items plus un par dépôt détecté, sans en-tête, ascenseur ni compteur ; et rien du tout d'exposé au système d'accessibilité.

* **Tâches :**
  * [x] Séparer la géométrie, exprimée en points de conception et projetée par un facteur fractionnaire, du choix des corps de police, qui ne peut être qu'entier. Allouer le tampon en pixels physiques, pour une présentation un pour un.
  * [x] Dessiner à la main une police bitmap en couverture de niveaux de gris, à avances proportionnelles, avec accents composés — ce qui lève la limite historique sur les capitales accentuées.
  * [x] Déplacer le panneau dans sa propre fenêtre, présentée par `softbuffer`, centrée sur l'écran du familier, déplaçable, et refermée sur `Échap` ou à la perte de focus.
  * [x] Router les événements de fenêtre par `WindowId`, jusqu'ici ignoré, et faire vivre la scène du familier pendant le réglage.
  * [x] Réécrire la composition du panneau : en-têtes de section, sous-titres, pastilles à largeur mesurée, ascenseur, compteur de résultats, état vide, descriptions sur plusieurs lignes.
  * [x] Introduire une navigation à deux niveaux et une recherche floue insensible aux diacritiques, classée par pertinence et globale depuis n'importe quel niveau.
  * [x] Brancher la souris : survol, clic, molette, déplacement de fenêtre.
  * [x] Exposer l'arbre sémantique du panneau au système via AccessKit, avec focus suivant la sélection et bascules annoncées comme des interrupteurs.
  * [x] Servir trois thèmes — sombre, clair, contraste renforcé — et vérifier leurs rapports de contraste par la suite de tests aux seuils WCAG 2.1.
  * [x] Persister les préférences d'interface : taille du texte, thème, mouvement réduit, fermeture à la perte de focus.
* **Livrable :** panneau net à toutes les densités, navigable au clavier comme à la souris, exposé aux lecteurs d'écran, et dont la lisibilité est verrouillée par des tests.

> **Écart assumé :** le panneau est présenté par `softbuffer` et non par une seconde surface `pixels`. Chaque instance de `pixels` construit son propre contexte wgpu — instance, adaptateur, device — et une seconde fenêtre aurait donc coûté un contexte graphique entier, de l'ordre de dix à vingt mégaoctets, ce qui aurait compromis la cible d'empreinte mémoire de la phase 10. Le panneau étant de toute façon composé en logiciel, `softbuffer` le présente par un simple transfert mémoire.

> **Écart assumé :** la pile d'accessibilité est isolée derrière la feature Cargo `a11y`, activée par défaut. Sous Linux, `accesskit_unix` entraîne la pile AT-SPI par D-Bus ; la feature permet de produire un binaire sans elle, et la CI vérifie les deux configurations.

> **Reste à faire :** le corps de police 11×20 déclaré par `FontSize` n'est pas encore dessiné. Le moteur sert le corps 8×15 à sa place, ce qui donne aux densités élevées un texte net mais un peu plus petit que la maquette ne le prévoit. Le sélecteur de corps est indépendant de cet état : les brancher ne demandera que du dessin.

> **Vérification manuelle non automatisable :** la forme de l'arbre d'accessibilité est couverte par des tests, mais ce qu'une voix de synthèse en fait ne l'est pas. Une passe au lecteur d'écran (NVDA, VoiceOver, Orca) reste nécessaire avant de considérer l'accessibilité comme validée.

> **Correction de la phase 2 :** la phase 2 annonçait une « transparence native OS » qui ne tenait pas sous Windows. Une surface graphique attachée à un HWND n'y propose aucun mode de composition alpha — vérifié sur les trois backends — et le familier s'affichait dans un carré noir de la taille de sa fenêtre. La présentation passe désormais par une fenêtre en couches (`WS_EX_LAYERED` + `UpdateLayeredWindow`), isolée dans `gremlin-system::platform::layered`, avec repli automatique sur le chemin GPU là où la surface honore l'alpha. Sous Windows, l'application ne crée plus aucun contexte graphique.

---

## Phase 7 : Surveillance d'activité & Outillage développeur (*Tooling & Process Watcher*) — livrée

**Objectif :** Élargir la perception de Gremlin aux outils de développement locaux (tests unitaires, builds, sessions de code) sans saturer le CPU.

* **Tâches :**
  * [x] Implémenter un observateur passif pour les suites de tests unitaires locales (`cargo test`, `npm test`, `pytest`, `go test`, `dotnet test`).
  * [x] Détecter les réussites et échecs de build/tests via la surveillance ciblée des répertoires de sortie et fichiers de compte-rendu.
  * [x] Intégrer de nouvelles récompenses d'XP et réactions émotionnelles : boost de joie sur tests passants, humeur paniquée / pansement / motivation sur tests brisés.
  * [x] Ajouter la détection d'inactivité de frappe et des sessions de code prolongées (*deep work*) pour adapter l'état de Gremlin.
  * [x] Encapsuler l'observateur dans `gremlin-watcher` avec des canaux bornés, sans polling agressif ni élévation de privilèges.
* **Livrable :** Gremlin réagit en temps réel à l'issue des tests et aux sessions de travail intensif en plus des commits Git.

---

## Phase 8 : Interactions bureau & Bien-être (*Desk Companion & Productivity*) — planifiée

**Objectif :** Transformer Gremlin en un véritable familier de bureau interactif favorisant la productivité et les pauses saines.

* **Tâches :**
  * [ ] Implémenter les interactions directes à la souris : caresses (clics affectueux), physique de gravité douce lors du lâcher après un déplacement.
  * [ ] Mettre en place un système de séries de productivité (*Streaks* de jours consécutifs de commits) avec déblocage de cosmétiques rares.
  * [ ] Créer un inventaire virtuel et des consommables (café pour remonter l'énergie, potion de debug, collation) activables par raccourci ou glisser-déposer.
  * [ ] Ajouter un mode Minuteur de concentration (*Pomodoro*) optionnel avec posture studieuse de Gremlin et rappels discrets d'étirement et d'hydratation.
  * [ ] Développer le magnétisme d'écran (ancrage fluide aux coins de l'écran ou au-dessus de la barre des tâches) et support multi-moniteurs robuste.
* **Livrable :** interactions directes avec le familier, gestion de séries et fonctionnalités de bien-être développeur.

---

## Phase 9 : Écosystème de Mods & Hub de Personnalisation (*Modding Hub & Packaging*) — planifiée

**Objectif :** Faciliter la création, le partage et la gestion sécurisée de packs de skins et d'accessoires communautaires.

* **Tâches :**
  * [ ] Définir un format d'archive de skin autonome `.gremlin` (packaging compressé avec vérification stricte contre les *zip bombs* et *path traversal*).
  * [ ] Développer un outil CLI de validation et prévisualisation d'assets (`cargo run -p gremlin-render --example validate_skin`) avec rapports d'erreurs clairs.
  * [ ] Implémenter la gestion de profils multiples (ex: profil professionnel vs projet personnel, avec associations de dossiers dédiées).
  * [ ] Ajouter un module audio chiptune / 8-bit procédural optionnel (sons rétro discrets, désactivés par défaut pour préserver le calme).
  * [ ] Permettre l'import/export de packs de skins en un clic depuis le menu de la barre des tâches ou par glisser-déposer de fichier `.gremlin`.
* **Livrable :** chaîne d'outillage complète pour les créateurs de skins et gestionnaire de profils utilisateur multi-environnements.

---

## Phase 10 : Métrologie, Signature & Distribution Certifiée (*Release & Quality Engineering*) — planifiée

**Objectif :** Garantir une distribution irréprochable avec des binaires signés, une consommation mesurée et des mises à jour fiables.

* **Tâches :**
  * [ ] Mettre en place un banc de mesure instrumenté des ressources (CPU < 0.1 % au repos, mémoire RSS < 35 Mo) validé en CI.
  * [ ] Configurer la signature de code et la notarisation sur les trois OS (Windows SmartScreen / Authenticode, macOS Notarization & Gatekeeper, paquets Linux Flatpak / AppImage / AUR).
  * [ ] Développer un tableau de bord local de statistiques hors-ligne (commits cumulés, historique des humeurs, temps de code, arbre généalogique d'évolution).
  * [ ] Implémenter un mécanisme de mise à jour automatique sécurisé (vérification cryptographique des sommes de contrôle, remplacement atomique avec reprise sur erreur).
  * [ ] Rédiger la documentation utilisateur finale, les guides de création de mods et les spécifications d'API.
* **Livrable :** binaires certifiés prêts pour une adoption à grande échelle, installables sans avertissement de sécurité OS.

---

## Perspectives futures & Recherche

* [ ] Intégration optionnelle avec des forges distantes (GitHub / GitLab / Gitea) pour réagir aux revues de code et fusions de PR (en mode local / token chiffré).
* [ ] Support des plugins WASM pour des comportements ou mini-jeux personnalisés isolés en bac à sable (*sandboxed runtime*).
* [ ] Mode multi-compagnons ou interaction locale en réseau local (P2P zero-config / mDNS) entre Gremlins d'une même équipe.
