# Gremlin — Le compagnon de bureau pour développeurs

> Un familier virtuel de bureau ultra-léger et autonome en Rust natif, qui grandit et évolue au rythme de vos commits Git. **Zéro configuration requise.**

---

## Points forts

* **Surveillance explicite et ciblée :** vous déclarez les dépôts que vous confiez à votre Gremlin, et lui n'observe qu'eux — précisément `.git/refs`, `.git/logs` et vos rapports de tests. Aucun parcours du disque, aucune racine devinée, aucun projet archivé réveillé par surprise. La réaction aux commits et aux bascules de branche passe par les interruptions du système de fichiers de l'OS : aucun hook manuel ni port réseau requis.
* **100 % natif :** Écrit en Rust pur avec rendu GPU accéléré (`winit` + `pixels`). La boucle d'événements ne fait jamais d'attente active : elle dort entre deux images (`ControlFlow::WaitUntil`) et n'est réveillée que par une image d'animation à afficher, une interaction ou un signal Git.
* **Accessible :** le panneau de paramètres expose son arbre sémantique au système (UI Automation, NSAccessibility, AT-SPI) et s'utilise donc au lecteur d'écran. Trois thèmes dont un à contraste renforcé, taille de texte réglable, mode mouvement réduit, et des rapports de contraste vérifiés par la suite de tests.
* **Cross-platform pur :** Fonctionne de manière native sur Windows, macOS et Linux sans empaquetage lourd ni Webview.
* **Forte personnalisation :** Moteur de rendu multi-calques modulaire (skins, chapeaux, accessoires, auras) extensible via de simples fichiers JSON et PNG.
* **Non intrusif :** Fenêtre transparente flottante sans bordure avec mode *click-through* pour coder sans gêne visuelle. La transparence est réellement par pixel : sous Windows elle passe par une fenêtre en couches, la seule voie possible (voir *Transparence de la fenêtre du familier*). Le panneau de paramètres occupe sa propre fenêtre : le familier reste visible et continue de s'animer pendant le réglage.
* **Résistant aux fichiers abîmés :** sauvegardes, configuration et manifests de skins sont validés au chargement. Une sauvegarde illisible est mise de côté plutôt qu'écrasée, et un manifest hostile ne peut ni figer ni faire planter l'application.
* **Outillage développeur passif :** Gremlin assimile les rapports JUnit, TRX, Jest JSON et son contrat JSON versionné. Il n'exécute aucune commande, n'installe aucun hook et ne lit ni les frappes ni la fenêtre active.
* **Familier manipulable :** un clic bref le caresse, un glisser le déplace, et il retombe doucement au bas de la zone de travail — barre des tâches et Dock exclus. Le placement survit au redémarrage, au changement d'écran et au changement de densité.
* **Séries de commits et bien-être :** les jours de travail sont reconstitués depuis le journal Git local, un petit inventaire de consommables remplace les soins illimités, et un minuteur de concentration optionnel propose des pauses sans jamais rien récompenser.

---

## Mécaniques de jeu

Gremlin possède un cycle de vie autonome régulé par une boucle d'état interne :

* **Énergie / Faim :** Alimentée par la fréquence des commits et l'activité de code. Baisse lentement avec l'inactivité.
* **Humeur / Joie :** Boostée par la régularité et les *pushes*. Chute en cas de dette technique ou d'inactivité prolongée. Les seuils comportent une hystérésis : une jauge oscillant autour d'un seuil ne provoque pas de clignotement d'humeur.
* **XP et Évolution :** Chaque commit rapporte de l'expérience, débloquant des formes évoluées (Bébé -> Adolescent -> Adulte -> Cyber-Gremlin) et de nouveaux cosmétiques. Seuls les vrais commits comptent : un `git checkout` ne rapporte pas d'XP.
* **États émotionnels :** Sprites dynamiques dédiés (*Happy*, *Coding*, *Hungry*, *Tired*, *Sick*, *Angry*, *Sleeping*, *Dead*).
* **Rattrapage hors-ligne :** au redémarrage, le temps écoulé depuis la dernière sauvegarde est simulé, dans la limite du plafond configuré.
* **Focus estimé :** après un commit ou un rapport reconnu, l'activité clavier/souris globale peut alimenter une estimation locale de session. Cette estimation est désactivable et ne progresse jamais si le compteur natif est indisponible.

---

## Séries de commits, inventaire et concentration

### Ce qu'est un « jour de commit »

Une série compte des **jours civils locaux**, pas des paquets de 24 heures. Le jour d'un commit est la date portée par son entrée de `.git/logs/HEAD`, avec le décalage horaire que Git y a inscrit **au moment du commit** : changer de fuseau ne réécrit donc jamais l'histoire.

Le journal local est la seule preuve retenue. L'historique des objets Git ne distingue pas un commit créé ici d'un commit simplement récupéré par un `git clone` : seul le reflog atteste d'une action sur cette machine. Les actions `commit`, `merge`, `cherry-pick`, `revert`, `am` et les rejeux de rebase comptent ; `checkout`, `pull`, `reset` et `branch` ne comptent pas.

Au rattachement d'un dépôt, Gremlin relit la fin de son journal — au plus 1 Mio, 20 000 lignes et 400 journées distinctes — pour reconstituer les jours passés. **Aucun XP n'est rejoué** : un historique prouve des journées de travail, il ne récompense pas une seconde fois des commits déjà faits. Un journal illisible laisse la série intacte et l'incident s'affiche dans le panneau.

### Les règles, en clair

| Situation | Effet |
| --- | --- |
| Plusieurs commits le même jour | une seule journée comptée |
| Un commit le lendemain | la série s'allonge |
| Le lendemain sans commit | la série reste affichée toute la journée — c'est la **grâce** |
| Le surlendemain sans commit | la série retombe à 0, le record est conservé |
| Un jour futur ou hors calendrier | ignoré |

Trois paliers — 3, 7 et 30 jours — débloquent définitivement trois cosmétiques : la feuille porte-bonheur, le casque de concentration et l'aura aurorale. Ils sont acquis une seule fois, survivent à une série rompue, et restent inéquipables tant qu'ils ne sont pas mérités — y compris si la sauvegarde est éditée à la main. Les accessoires ordinaires et les mods ne sont soumis à aucun palier.

Le jour courant est **injecté** dans le moteur de jeu, jamais lu par lui : c'est ce qui rend ces règles éprouvables autour de minuit, en fin de mois ou un 29 février, sans toucher à l'heure de la machine. La date locale vient de `gremlin-system::calendar`, seule à connaître le fuseau et l'heure d'été.

### Inventaire

Le premier commit de chaque journée rapporte un objet, en rotation déterministe : collation, café, potion de debug. Trois types, neuf exemplaires au plus par type, et rien d'autre — ni boutique, ni monnaie, ni échange.

| Objet | Effet nominal |
| --- | --- |
| Café | +25 énergie |
| Potion de debug | +15 sur les trois jauges |
| Collation | +25 satiété |

Les gains sont **nominaux** : les jauges restant plafonnées à 100, un café bu à 95 d'énergie n'en rend que 5, et c'est cet effet réel qui est affiché. Un objet sans effet possible, un stock vide ou un familier endormi produisent un refus explicite — et le stock reste intact, la transaction validant avant de muter.

Ces objets **remplacent** les anciennes actions « nourrir » et « soigner », qui étaient illimitées : les garder à côté aurait rendu l'inventaire décoratif. Caresser et réanimer restent des actions directes, elles ne consomment rien.

Trois façons de consommer un objet, toutes équivalentes : `Entrée` sur sa ligne, les raccourcis `1`, `2`, `3` dans le groupe Productivité, ou un glisser de la ligne vers l'aperçu du familier. Relâcher hors de l'aperçu annule le geste sans rien décrémenter.

### Minuteur de concentration

Désactivé par défaut. L'activer laisse le minuteur **à l'arrêt** : aucun temps mesuré ni rappel affiché avant un démarrage explicite. Le cycle est de 25 minutes de travail, 5 minutes de pause, et une pause longue de 15 minutes tous les quatre blocs.

Le démarrage de chaque phase reste volontaire : à la fin d'un bloc, la pause est *proposée*, pas lancée. Une pause peut être passée ; un bloc de travail, non — il serait comptabilisé sans avoir été accompli.

Le minuteur n'avance que sur du temps **réellement vécu par le processus**. Le rattrapage hors-ligne ne le fait pas progresser, une suspension de la machine le met en pause plutôt que de sauter des phases, et une session rechargée depuis une sauvegarde reprend en pause : Gremlin ne prétend pas qu'un processus arrêté a mesuré une session de travail.

Il n'accorde ni XP, ni objet, ni jour de série. C'est délibéré : le minuteur favorise la santé, pas l'accumulation. Les seuls retours sont une posture studieuse du familier et des rappels d'étirement et d'hydratation, en bulles discrètes qu'une alerte vitale masque toujours.

---

## Interactions directes et placement

Le mode d'interaction est **exclusif** du mode *click-through* : quand la souris traverse la fenêtre, celle-ci ne reçoit aucun événement de pointeur, et les gestes directs sont indisponibles par définition. Le panneau le dit ainsi plutôt que de promettre les deux à la fois ; la commande « Caresser » reste l'équivalent clavier et lecteur d'écran.

En mode interactif, un appui sur un **pixel visible** du familier arme un geste :

* un relâchement bref — moins de 500 ms — et immobile produit une **caresse**, une seule ;
* un déplacement au-delà de six points de conception produit un **glisser**, puis une chute au relâchement ;
* une perte de focus, une fermeture de fenêtre ou un refus du système annule le geste sans action métier, et l'incident est remonté au panneau.

Le seuil est exprimé en points de conception et projeté en pixels physiques : six points restent six points à 100 % comme à 200 %. La hitbox suit le masque alpha du corps et des accessoires — cliquer un cœur qui s'éloigne ne compte pas, et un clic dans un coin transparent ne fait rien. Cela ne rend pas pour autant la zone transparente traversable pixel par pixel : en mode interactif, la fenêtre native reste rectangulaire.

Au relâchement, le familier retombe vers le bas de la **zone de travail** de son écran — celle qui exclut la barre des tâches, le Dock ou un panneau — avec un rebond amorti au plus. En mouvement réduit, il s'y place instantanément. L'ancrage aux coins est activable séparément.

Ce qui est persisté est une **intention** — un bord, une position en millièmes le long de ce bord, une empreinte d'écran — jamais une coordonnée absolue : un écran débranché ou une définition changée rendrait une coordonnée absurde, quand une ancre se reprojette toujours.

### Ce que chaque système permet réellement

| Plateforme | Zone de travail | Placement |
| --- | --- | --- |
| Windows | `GetMonitorInfoW` / `rcWork` | complet |
| macOS | `NSScreen::visibleFrame` | complet |
| Linux / X11 | `_NET_WORKAREA`, repli marqué sur les limites du moniteur | complet |
| Linux / Wayland | non publiée | **indisponible** |

Un client Wayland ordinaire ne connaît ni sa position globale, ni celle des autres surfaces, et ne choisit pas librement où sa fenêtre apparaît. Gremlin y désactive le magnétisme et le dit dans le panneau, plutôt que d'afficher un réglage sans effet. Là où un gestionnaire de fenêtres X11 ne publie pas `_NET_WORKAREA`, les limites du moniteur servent de repli et l'écart est marqué comme tel.

Le support multi-écrans est dit robuste parce qu'il choisit l'écran par plus grande intersection, accepte les coordonnées négatives, survit au débranchement d'un moniteur et ne laisse jamais la fenêtre hors écran — pas parce qu'il simulerait une capacité absente du protocole.

---

## Rapports de tests et builds

Gremlin observe uniquement des résultats explicites écrits dans un dépôt déjà surveillé. Une commande nue comme `cargo test`, `npm test`, `pytest`, `go test` ou `dotnet test` sans reporter ne fournit pas toujours un résultat final exploitable : Gremlin ne tente donc jamais de le deviner à partir d'un cache, d'un binaire ou d'un dossier de sortie.

Les emplacements reconnus par défaut sont `target/nextest/`, `test-results/`, `TestResults/`, `.gremlin/results/` et le fichier racine `junit.xml`. Quelques exemples de production de rapports :

```bash
pytest --junitxml=junit.xml
```

```bash
gotestsum --junitfile junit.xml
```

```bash
jest --json --outputFile=test-results/jest.json
```

```bash
dotnet test --logger "trx;LogFileName=results.trx"
```

Pour `cargo-nextest`, un profil peut écrire son JUnit sous `target/nextest/`, par exemple `target/nextest/default/junit.xml`. Les reporters JUnit de Vitest, Mocha et autres outils peuvent écrire dans `test-results/` ou `junit.xml`.

Les builds ne sont jamais inférés. Un script ou un outil peut déposer ce contrat JSON v1 sous `.gremlin/results/` :

```json
{
  "schema_version": 1,
  "run_id": "2026-08-24T12:00:00Z-42",
  "kind": "build",
  "tool": "cargo",
  "outcome": "passed",
  "duration_ms": 4210
}
```

Pour `kind: "test"`, ajouter `passed`, `failed` et éventuellement `skipped`; `outcome` doit rester cohérent avec ces compteurs. `run_id` identifie un run et empêche sa double assimilation.

Les trois bascules « Rapports de tests et builds », « Estimation des sessions de focus » et « Rappels de pause » se trouvent dans les préférences système du panneau. Les chemins supplémentaires peuvent être déclarés dans `watcher.tooling_sources`; ils doivent rester relatifs au dépôt. Les plateformes prises en charge pour l'inactivité sont Windows, macOS et Linux/X11. Wayland pur, XWayland seul et les environnements headless affichent explicitement l'indisponibilité au lieu de fabriquer du temps de focus.

---

## Cadencement et consommation

Il n'y a pas de fréquence d'affichage fixe : le rythme de réveil s'adapte au contexte, et une image n'est recomposée que si quelque chose a réellement changé.

| Contexte | Intervalle de réveil |
| --- | --- |
| Glisser-déposer de la fenêtre | 33 ms (~30 im/s) |
| Chute en cours après un lâcher | 16 ms, jusqu'à stabilisation |
| Animation en cours | durée de l'image suivante, plafonnée à 60 im/s |
| Aucune animation en attente | 1 s |
| Panneau ouvert | resserre l'intervalle ci-dessus à 100 ms au plus |
| Panneau ouvert, minuteur en cours | resserre à 1 s pour le compte à rebours |
| Panneau ouvert, mouvement réduit | aucun resserrement : réveil sur événement seul |

Le panneau **resserre** la cadence sans la remplacer : il occupe sa propre fenêtre, et le familier continue donc de s'animer à son rythme pendant le réglage. Ce resserrement n'a qu'une raison d'être — faire clignoter le curseur de saisie ; le mode mouvement réduit l'éteint et le supprime avec lui.

La chute impose son propre pas **le temps qu'elle dure**, puis disparaît : une fois le familier posé, plus aucune cadence physique ne subsiste. Le compte à rebours du minuteur ne se rafraîchit qu'à la seconde, et seulement quand le panneau est ouvert pour l'afficher — le mesurer plus finement ne changerait rien à l'écran. Le changement de jour civil, lui, est vérifié lors du réveil de simulation qui existe déjà : il n'ajoute aucune horloge.

Les signaux Git et les modifications de skins ne subissent pas cette latence : ils réveillent la boucle immédiatement via le proxy de la boucle d'événements.

---

## Transparence de la fenêtre du familier

La fenêtre du familier est déclarée transparente, mais c'est la **surface de présentation** qui décide si le canal alpha est honoré — et sous Windows, elle ne l'honore pas.

Une surface graphique attachée à un HWND classique n'offre aucun mode de composition alpha. Mesuré sur une GeForce RTX 3080, les trois backends répondent la même chose :

```
Vulkan  : modes = [Opaque]   DX12 : modes = [Opaque]   OpenGL : modes = [Opaque]
```

Aucune alternative n'est proposée. Tout pixel laissé transparent est donc aplati en noir, et le familier apparaît dans un carré noir de la taille exacte de sa fenêtre. Le diagnostic est reproductible :

```bash
cargo run -p gremlin-app --example probe_surface_alpha
```

La voie retenue est la **fenêtre en couches** (`WS_EX_LAYERED` + `UpdateLayeredWindow`), qui accepte un canal alpha et laisse le gestionnaire de fenêtres composer correctement. Elle convient d'autant mieux que le familier est déjà composé dans un tampon mémoire : c'est exactement ce que cette interface attend. Sous Windows, l'application ne crée donc **aucun contexte graphique** — ni pour le familier, ni pour le panneau.

Sur macOS et sur les environnements Linux dotés d'un compositeur, la surface graphique propose un mode honorant l'alpha : le chemin GPU y reste en place, et la présentation en couches n'est pas utilisée. Le choix est automatique et sans configuration.

Le format de pixel exigé par Windows — BGRA à alpha prémultiplié — est converti par une fonction pure, compilée et testée sur les trois systèmes : c'est là que se cachent les vraies erreurs, inversion de canaux ou halo clair sur les contours.

---

## Panneau de paramètres

Le panneau s'ouvre par `Espace` ou un clic droit sur le familier, et depuis la zone de notification. Il s'inspire de Raycast : une seule barre de recherche, tout au clavier, et un aperçu vivant à droite qui essaie l'accessoire survolé sur le familier en temps réel.

### Navigation à deux niveaux

La racine énumère six groupes — Profil, Soins et actions, Garde-robe, Dépôts surveillés, Productivité, Préférences système — chacun avec son décompte. Cette structure existe parce que la liste plate devenait impraticable : entre les accessoires, les réglages et les dépôts, quelques dizaines de lignes suffisaient à noyer tout le reste.

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
| `Suppr` | retirer l'élément de la ligne, là où elle porte une corbeille |
| `1` `2` `3` | consommer café, potion ou collation, dans le groupe Productivité et hors saisie |
| `Ctrl+S` | sauvegarder immédiatement |

Les raccourcis numériques ne s'activent que dans le groupe Productivité et lorsque la recherche est vide : partout ailleurs ils restent du texte, sans quoi un dépôt nommé « projet2 » deviendrait introuvable.

À la souris : survol, clic pour activer, molette pour défiler, et glisser depuis une zone vide pour déplacer la fenêtre — elle n'a pas de barre de titre. La corbeille d'une ligne de dépôt se clique directement.

`Ctrl+A` n'est volontairement pas lié : sans modèle de sélection de texte, il ne pourrait que mentir sur son effet.

### Dépôts surveillés

Gremlin ne surveille que les dépôts que vous lui confiez. Le groupe **Dépôts surveillés** de la palette porte trois façons d'en ajouter un :

* `Ajouter un dépôt Git…` ouvre une saisie : le champ de recherche devient un champ de chemin, et la ligne dit en direct si le chemin désigne un dépôt Git valide. `Entrée` confirme, `Échap` abandonne.
* `Parcourir un dossier…` ouvre le sélecteur de dossier du système. L'entrée n'apparaît que là où ce sélecteur existe — voir la feature `folder_dialog` plus bas.
* `Ajouter le dossier courant` n'apparaît que si Gremlin a été lancé depuis un dépôt Git qu'il ne suit pas déjà.

Chaque dépôt suivi occupe ensuite une ligne : `Entrée` ouvre son dossier, la **corbeille** à droite — ou la touche `Suppr` — le retire. Un dépôt devenu introuvable (disque débranché, dossier déplacé) reste listé, marqué `INDISPONIBLE` avec sa cause : il ne disparaît pas tout seul de votre configuration, et reste donc retirable.

La liste vit dans `watcher.tracked_repos` du fichier de sauvegarde, et s'édite aussi à la main.

### Netteté sur écran à haute densité

Le tampon du panneau est alloué en pixels **physiques**, calculés depuis le facteur d'échelle du système, et présenté par un transfert un pour un. À 125 % ou 150 %, aucun rééchantillonnage n'intervient donc. La géométrie suit le facteur en continu ; la police, elle, est bitmap et ne s'agrandit que par facteurs entiers — d'où le réglage explicite de **taille du texte**, qui rend la main là où la mise à l'échelle automatique ne peut atteindre qu'un palier voisin.

### Accessibilité

* **Lecteur d'écran :** l'arbre sémantique du panneau est exposé au système via AccessKit. Le focus suit la sélection, chaque déplacement au clavier est donc annoncé ; les réglages à deux états sont annoncés comme des interrupteurs, avec leur état.
* **Thèmes :** sombre, clair, suivi du système, et contraste renforcé. Les rapports de contraste de chaque paire texte-sur-fond sont **vérifiés par la suite de tests** aux seuils WCAG 2.1 (4,5:1 pour le texte, 3:1 pour les composants) : une régression de lisibilité ne peut pas être commise.
* **Mouvement réduit :** fige le curseur de saisie, seule animation permanente du panneau.
* **Sélection et survol** sont signalés par un liseré d'accent — plein pour la sélection, de demi-largeur pour le survol — et non par une teinte de fond seule.

La pile d'accessibilité est isolée derrière la feature Cargo `a11y`, activée par défaut. `cargo build --no-default-features` produit un binaire sans elle.

### Features Cargo

| Feature | Par défaut | Effet |
| --- | --- | --- |
| `a11y` | oui | expose l'arbre sémantique du panneau à l'OS (UI Automation, `NSAccessibility`, AT-SPI) |
| `folder_dialog` | oui | ajoute l'entrée « Parcourir un dossier… », qui ouvre le sélecteur natif |

Une boîte de dialogue de fichiers appartient à l'environnement de bureau : elle ne se dessine pas dans un tampon de pixels. La dépendance qui l'ouvre est donc acceptée, mais confinée derrière cette feature et appelée depuis le seul module `desktop.rs`. Elle est servie sur un fil dédié, la boîte étant modale : la boucle d'événements ne s'y bloque jamais.

**macOS en est exclu**, et l'entrée n'y apparaît pas : `NSOpenPanel` doit s'exécuter sur le fil principal, et l'y renvoyer depuis un fil secondaire laisse un risque d'interblocage qu'un démon résident ne peut pas se permettre. La saisie du chemin, elle, fonctionne partout.

### Planches de contrôle

La police et la mise en page sont dessinées à la main : elles se jugent en les regardant, pas en relisant des coordonnées. Deux exemples les rendent hors écran vers des fichiers PNG :

```bash
cargo run -p gremlin-app --example font_proof_sheet
```

```bash
cargo run -p gremlin-app --example panel_proof_sheet
```

Les accessoires se jugent de la même façon. Une troisième planche les rend sur les trois morphologies, sur toutes les humeurs, puis déroule frame par frame les quatre animations :

```bash
cargo run -p gremlin-render --example accessory_proof_sheet
```

La planche du panneau couvre les états de la phase 8 : inventaire fourni comme vide, objet en cours de glisser sur la cible et à côté, série à 0, 7 et 30 jours, récompense acquise comme verrouillée, minuteur en travail et en pause, et placement annoncé indisponible — chacun dans les thèmes concernés. Générer les images ne vaut pas validation : il faut les ouvrir.

---

## Stack technique

* **Langage :** Rust (édition 2021, version minimale 1.92)
* **Gestionnaire de fenêtres :** `winit` (borderless, click-through via `set_cursor_hittest`)
* **Transparence du familier :** fenêtre en couches Win32 (`UpdateLayeredWindow`) sous Windows, surface graphique à composition alpha ailleurs
* **Moteur de rendu 2D :** `pixels` / `wgpu` pour le familier sur les plateformes où la surface graphique honore l'alpha ; sous Windows, aucun contexte GPU n'est créé
* **Dates et fuseaux locaux :** `jiff`, confiné à `gremlin-system::calendar` — la base IANA n'est pas une chose qui se réimplémente
* **Présentation du panneau :** `softbuffer` (transfert mémoire sans GPU). Chaque instance de `pixels` construit son propre contexte wgpu : une seconde fenêtre en `pixels` aurait coûté un contexte graphique entier, contre l'objectif d'empreinte mémoire du projet.
* **Accessibilité :** `accesskit` et `accesskit_winit`, derrière la feature `a11y`
* **Sélecteur de dossier :** `rfd`, derrière la feature `folder_dialog` (portail XDG sous Linux, pas de liaison GTK)
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
| `gremlin-system` | Fenêtre, zone de notification, autostart, chemins, stockage atomique, calendrier local et topologie des écrans. |
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

Les sprites sont dessinés sur une toile complète de 64×64 pixels. Chaque accessoire indique son point d'attache source (`anchor`) ; le compositeur l'aligne sur l'ancre du skin actif (`hat`, `glasses`, `outfit`, `held` ou `aura`). Le skin peut déclarer des `anchor_offsets_per_mood` par calque, ou via les groupes `head` et `body`, afin que l'équipement suive aussi les changements de pose. Une tenue ajustée peut activer `clip_to_body` pour être découpée sur l'alpha du corps courant ; le champ reste désactivé par défaut afin de préserver les capes et anciens mods. En l'absence des nouveaux champs, les coordonnées du skin classique et les ancres globales restent le repli compatible. Les points `head` et `effect_origin` positionnent aussi les bulles et particules. Les dimensions, durées et coordonnées déclarées sont bornées au chargement, de sorte qu'un manifest tiers ne puisse ni figer le rendu ni provoquer une allocation démesurée.

### Accessoires intégrés

Les treize accessoires officiels sont des sprites 64×64 rangés sous `assets/accessories/builtin/`, un dossier par identifiant, embarqués dans l'exécutable à la compilation. Ils sont décodés une seule fois au démarrage : un pack dont une frame manque, ne se décode pas ou sort entièrement transparente est journalisé et ignoré en bloc, plutôt que d'exposer un accessoire à moitié chargé.

Chaque manifest d'accessoire porte des frames et un point d'attache communs, puis une table `variants` indexée par famille visuelle. Le skin déclare la sienne avec `accessory_style` — `default`, `baby` ou `evolved` ; toute autre valeur, comme une absence, retombe sur `default`. Un chapeau dessiné pour le crâne rond du bébé lui est ainsi servi sans qu'aucun accessoire n'ait à embarquer une variante par humeur : les changements de pose restent portés par les ancres du skin. Une variante vide ou absente réutilise les frames communes, si bien qu'un mod écrit avant cette table continue de fonctionner sans migration.

Quatre accessoires sont animés : le scintillement du chapeau d'archimage (3 frames), le balayage de la visière (2), la vapeur du mug (3) et le défilement de la pluie de code (4). En mode mouvement réduit, ils sont figés sur leur première frame.

Trois d'entre eux — la feuille porte-bonheur, le casque de concentration et l'aura aurorale — sont des **récompenses de série** : le catalogue les porte comme les autres, et c'est l'orchestrateur qui refuse de les équiper tant que le palier n'est pas atteint. Leur dessin est reproductible :

```bash
cargo run -p gremlin-render --example generate_streak_rewards
```

---

## Démarrage rapide

### Prérequis

* [Rust et Cargo](https://rustup.rs/) 1.92 ou plus récent
* Sous Linux : `libxdo-dev`, `libxss1`, `libayatana-appindicator3-dev`, `libgtk-3-dev`, `libasound2-dev`

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
- [x] **Phase 3** : Surveillance passive des dépôts Git déclarés explicitement
- [x] **Phase 4** : Composition multi-calques et moteur d'accessoires (*Asset Engine*)
- [x] **Phase 5** : Intégration système, zone de notification et publication release (*System UX & Release*)
- [x] **Phase 6** : Animations avancées, micro-particules et phylactères (*Dynamic FX & Layer Animation*)
- [x] **Phase 6bis** : Refonte du panneau de paramètres et accessibilité système (*Accessible Panel*)
- [x] **Phase 7** : Surveillance des tests unitaires et outillage développeur (*Tooling & Focus Watcher*)
- [x] **Phase 8** : Interactions bureau, séries de productivité et bien-être (*Desk Companion & Productivity*)
- [ ] **Phase 9** : Écosystème de mods, packaging `.gremlin` et validation CLI (*Modding Hub*)
- [ ] **Phase 10** : Métrologie, signature multi-OS et distribution certifiée (*Release Engineering*)

Consultez [ROADMAP.MD](ROADMAP.MD) pour le détail exhaustif de chaque phase et ses livrables.

---

## Licence

Distribué sous licence MIT.
