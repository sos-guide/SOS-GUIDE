/* SOS-GUIDE — Préférences UI UNIFIÉES et persistantes (source unique) :
   1) thème clair/sombre, 2) taille du texte (zoom a11y).
   À charger en `<script src="/lib/sos-theme.js"></script>` juste après <body>
   (applique le thème ET la taille avant peinture, évite le flash). Persistance
   dans localStorage sous des clés communes à toutes les pages. Câble le bouton de
   nav `#themeToggle` s'il existe, sinon injecte un bouton flottant `.theme-btn` ;
   injecte un contrôle A−/A+ de taille du texte (sauf si le <body> porte
   l'attribut `data-no-a11y-zoom`, ex. admin.html). */
(function () {
  var KEY = 'sos_guide_theme';
  var TKEY = 'sos_guide_textsize';
  // 4 niveaux (WCAG 1.4.4 : jusqu'à 200 % ; on échelonne le rem racine).
  var SIZES = ['87.5%', '100%', '112.5%', '125%'];
  var LABELS = ['Petit', 'Normal', 'Grand', 'Très grand'];
  function isLight() {
    return !!(document.body && document.body.classList.contains('light-mode'));
  }
  function saved() {
    try {
      return (
        localStorage.getItem(KEY) ||
        (window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light')
      );
    } catch (e) {
      return 'dark';
    }
  }
  function syncButtons(light) {
    ['themeToggle', 'sosThemeBtn'].forEach(function (id) {
      var b = document.getElementById(id);
      if (b) b.textContent = light ? '☀️' : '🌙';
    });
  }
  function apply(light) {
    if (document.body) document.body.classList.toggle('light-mode', light);
    try {
      localStorage.setItem(KEY, light ? 'light' : 'dark');
    } catch (e) {}
    syncButtons(light);
  }
  function toggle() {
    apply(!isLight());
  }

  // ---- Taille du texte (zoom a11y) ----
  function savedSize() {
    try {
      var n = parseInt(localStorage.getItem(TKEY), 10);
      return isNaN(n) ? 1 : Math.max(0, Math.min(SIZES.length - 1, n));
    } catch (e) {
      return 1;
    }
  }
  var sizeIdx = savedSize();
  function applySize(idx) {
    sizeIdx = Math.max(0, Math.min(SIZES.length - 1, idx));
    // Échelle du rem racine : toutes les tailles en rem suivent (texte du contenu).
    document.documentElement.style.fontSize = SIZES[sizeIdx];
    try {
      localStorage.setItem(TKEY, String(sizeIdx));
    } catch (e) {}
    var lvl = document.getElementById('a11yLevel');
    if (lvl) {
      lvl.textContent = SIZES[sizeIdx];
      lvl.setAttribute('aria-label', 'Taille du texte : ' + LABELS[sizeIdx]);
    }
  }
  function bumpSize(delta) {
    applySize(sizeIdx + delta);
  }

  window.SOSTheme = { apply: apply, toggle: toggle, setSize: applySize };

  // Applique immédiatement (le script est placé juste après <body>).
  apply(saved() === 'light');
  applySize(sizeIdx);

  // À DOM prêt : câble le toggle de nav, ou pose un bouton flottant si absent.
  document.addEventListener('DOMContentLoaded', function () {
    var nav = document.getElementById('themeToggle');
    if (nav) {
      nav.addEventListener('click', toggle);
    } else if (!document.getElementById('sosThemeBtn')) {
      var b = document.createElement('button');
      b.id = 'sosThemeBtn';
      b.className = 'theme-btn';
      b.type = 'button';
      b.setAttribute('aria-label', 'Basculer le thème clair/sombre');
      b.addEventListener('click', toggle);
      document.body.appendChild(b);
    }
    syncButtons(isLight());

    // Contrôle A−/A+ de taille du texte : câble les boutons de la barre du haut
    // (`#a11yMinus`/`#a11yPlus`) s'ils existent, sinon injecte un widget flottant.
    var am = document.getElementById('a11yMinus');
    var ap = document.getElementById('a11yPlus');
    if (am && ap) {
      am.addEventListener('click', function () {
        bumpSize(-1);
      });
      ap.addEventListener('click', function () {
        bumpSize(1);
      });
    } else if (
      !document.getElementById('a11yZoom') &&
      !(document.body && document.body.hasAttribute('data-no-a11y-zoom'))
    ) {
      // `data-no-a11y-zoom` (ex. admin.html) : la taille globale reste appliquée,
      // mais on n'injecte pas le widget flottant A−/A+ — page de gestion, pas
      // d'usage public où l'agrandissement importe.
      var box = document.createElement('div');
      box.id = 'a11yZoom';
      box.className = 'a11y-zoom';
      box.setAttribute('role', 'group');
      box.setAttribute('aria-label', 'Taille du texte');
      var minus = document.createElement('button');
      minus.type = 'button';
      minus.textContent = 'A−';
      minus.setAttribute('aria-label', 'Réduire la taille du texte');
      minus.addEventListener('click', function () {
        bumpSize(-1);
      });
      var lvl = document.createElement('span');
      lvl.id = 'a11yLevel';
      lvl.className = 'a11y-zoom-level';
      lvl.setAttribute('aria-live', 'polite');
      lvl.textContent = SIZES[sizeIdx];
      lvl.setAttribute('aria-label', 'Taille du texte : ' + LABELS[sizeIdx]);
      var plus = document.createElement('button');
      plus.type = 'button';
      plus.textContent = 'A+';
      plus.setAttribute('aria-label', 'Augmenter la taille du texte');
      plus.addEventListener('click', function () {
        bumpSize(1);
      });
      box.appendChild(minus);
      box.appendChild(lvl);
      box.appendChild(plus);
      document.body.appendChild(box);
    }
  });
})();
