// ===== ROVER central backend URLs (single source of truth) =====
// Backend now serves API + WS + dashboard on ONE port (8000).
// Same-origin when opened via http://<server>:8000/ (recommended).
// Legacy fallback: opened via old :8080 static server -> API lives on :8000.
window.ROVER = (function () {
  var proto = window.location.protocol || 'http:';
  var host = window.location.hostname || 'localhost';
  var port = window.location.port || '';
  var apiBase, wsUrl;
  if (port === '8080') {
    apiBase = proto + '//' + host + ':8000';
    wsUrl = (proto === 'https:' ? 'wss://' : 'ws://') + host + ':8000/ws';
  } else {
    apiBase = proto + '//' + window.location.host;
    wsUrl = (proto === 'https:' ? 'wss://' : 'ws://') + window.location.host + '/ws';
  }
  if (proto === 'file:') {
    apiBase = 'http://localhost:8000';
    wsUrl = 'ws://localhost:8000/ws';
  }
  return { apiBase: apiBase, wsUrl: wsUrl };
})();

(function() {
  // Page-transition fallback flag: browsers without the View Transitions API
  // get the .no-vt CSS fade-in instead of instant switches.
  if (!('startViewTransition' in document)) {
    document.documentElement.classList.add('no-vt');
  }

  var header = document.getElementById('site-header');
  if (header) {
    var ticking = false;
    window.addEventListener('scroll', function() {
      if (!ticking) {
        window.requestAnimationFrame(function() {
          if (window.scrollY > 10) {
            header.classList.add('scrolled');
          } else {
            header.classList.remove('scrolled');
          }
          ticking = false;
        });
        ticking = true;
      }
    });
  }

  // ===== Edge strip: manual dock toggle (no auto-hide) =====
  var dock = document.getElementById('nav-dock');
  var edgeToggle = document.getElementById('dock-edge-toggle');
  var edgeLock = document.getElementById('dock-edge-lock');
  // Open/closed persists across pages so the dock behaves as one continuous
  // element, chevron included. An explicit lock still overrides on load.
  var dockOpenStored = localStorage.getItem('dock-open');
  var dockHidden = dockOpenStored === 'false'; // default: visible
  var dockLocked = localStorage.getItem('dock-locked') === 'true';
  var navigating = false;   // guards double-tap dock navigation

  function setDockVisible(visible, persist) {
    if (!dock) return;
    dockHidden = !visible;
    dock.classList.toggle('dock-hidden', !visible);
    if (persist !== false) {
      localStorage.setItem('dock-open', visible ? 'true' : 'false');
    }
    // Edge strip stays reachable so the dock can always be toggled back.
    // Chevron and dock always agree, on every page load and every toggle.
    if (edgeToggle) {
      var icon = edgeToggle.querySelector('svg');
      if (icon) {
        icon.innerHTML = visible ? '<path d="m15 18-6-6 6-6"/>' : '<path d="m9 18 6-6-6-6"/>';
      }
    }
    if (!dockLocked) {
      dock.classList.toggle('dock-locked', false);
    }
  }

  function applyLockState() {
    if (!dock || !edgeLock) return;
    if (dockLocked) {
      dock.classList.add('dock-locked');
      dockHidden = true;
      var icon = edgeLock.querySelector('svg');
      if (icon) icon.innerHTML = '<rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>';
      edgeLock.title = 'Unlock dock';
    } else {
      dock.classList.remove('dock-locked');
      var icon = edgeLock.querySelector('svg');
      if (icon) icon.innerHTML = '<rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 9.9-1"/>';
      edgeLock.title = 'Lock dock hidden';
    }
  }

  // Apply initial state: explicit lock wins, otherwise the stored open/closed
  // choice (default open). Visibility and chevron agree from the first paint.
  setDockVisible(!dockHidden && !dockLocked, false);
  if (dockLocked) applyLockState();

  // Empty-space tap closes the dock (lock still exempts). Attached at init
  // because the dock loads open; dock/edge taps are excluded, so toggling
  // can never instantly re-close itself.
  function onDocumentClick(e) {
    if (dockLocked) return;
    var edge = document.getElementById('dock-edge');
    if (dock && !dock.contains(e.target) && edge && !edge.contains(e.target)) {
      setDockVisible(false);
    }
  }
  document.addEventListener('click', onDocumentClick);

  if (edgeToggle && dock) {
    edgeToggle.addEventListener('click', function() {
      if (dockLocked) return;
      setDockVisible(dockHidden);
    });
  }

  if (edgeLock) {
    edgeLock.addEventListener('click', function() {
      dockLocked = !dockLocked;
      localStorage.setItem('dock-locked', dockLocked);
      if (dockLocked) {
        setDockVisible(false);
      }
      applyLockState();
    });
  }

  // ===== Dock nav items =====
  var dockItems = document.querySelectorAll('.dock-item');
  var indicator = document.getElementById('dock-indicator');

  function getItemOffset(item) {
    if (!item || !item.parentElement) return 0;
    var parent = item.parentElement;
    var items = parent.querySelectorAll('.dock-item');
    var index = Array.prototype.indexOf.call(items, item);
    if (index === -1) return 0;
    var h = 42;
    var gap = 12;
    return index * (h + gap);
  }

  // Places the highlight instantly when true (page load: no phantom glide
  // from the top), glides it when false (dock tap before navigating).
  function updateIndicator(instant) {
    if (!indicator) return;
    var active = document.querySelector('.dock-item.active');
    if (active) {
      var y = 'translateY(' + getItemOffset(active) + 'px)';
      if (instant) {
        var prev = indicator.style.transition;
        indicator.style.transition = 'none';
        indicator.style.transform = y;
        void indicator.offsetHeight;
        indicator.style.transition = prev;
      } else {
        indicator.style.transform = y;
      }
    }
  }

  // Glide-then-navigate: let the highlight slide to the tapped icon before
  // the wipe fires, so the dock motion always completes instead of dying
  // mid-flight on unload. Same-page taps do nothing. Double-taps guarded.
  var NAV_GLIDE_MS = 300;
  dockItems.forEach(function(item) {
    item.addEventListener('click', function() {
      if (navigating) return;
      var href = this.getAttribute('data-href');
      var target = href ? new URL(href, window.location.href).pathname : null;
      if (target && target === window.location.pathname) return;
      dockItems.forEach(function(n) { n.classList.remove('active'); });
      this.classList.add('active');
      updateIndicator();
      if (href) {
        navigating = true;
        setTimeout(function() { window.location.href = href; }, NAV_GLIDE_MS);
      }
    });
  });

  // ===== Highlight current page =====
  var path = window.location.pathname;
  dockItems.forEach(function(item) {
    var href = item.getAttribute('data-href');
    if (href) {
      var hrefPath = new URL(href, window.location.href).pathname;
      if (path.indexOf(hrefPath) !== -1) {
        dockItems.forEach(function(n) { n.classList.remove('active'); });
        item.classList.add('active');
      }
    }
  });
  updateIndicator(true);
})();

// ===== Command palette (Ctrl+K) — the search bar, usable =====
// Shared by every page: fuzzy list over pages, motors and actions.
// Motors deep-link with ?motor=N (honoured by detailed + test pages).
// Actions navigate and preselect — they never fire hardware by themselves.
(function() {
  'use strict';

  var MOTOR_SHORT = ['FL', 'FR', 'RL', 'RR'];
  var MOTOR_FULL = ['Front Left', 'Front Right', 'Rear Left', 'Rear Right'];

  // Relative prefix from the current page depth: '/' -> './', '/x/' -> '../'
  function prefix() {
    var depth = window.location.pathname.split('/').filter(function (s) { return s; }).length;
    return depth > 0 ? '../' : './';
  }

  function pageEntries() {
    // Mirror the dock so palette destinations never drift from navigation.
    var out = [];
    document.querySelectorAll('.dock-item[data-href]').forEach(function (el) {
      var href = el.getAttribute('data-href');
      var title = el.getAttribute('title') || href;
      out.push({
        group: 'Pages',
        title: 'Go to ' + title,
        keywords: (title + ' page open go').toLowerCase(),
        href: href
      });
    });
    // Setup has no dock entry but is a real destination.
    out.push({
      group: 'Pages',
      title: 'Go to Setup',
      keywords: 'setup settings config broker token page',
      href: prefix() + 'setup/'
    });
    return out;
  }

  function motorEntries() {
    var out = [];
    MOTOR_SHORT.forEach(function (short, i) {
      var id = i + 1;
      var kw = (short + ' ' + MOTOR_FULL[i] + ' motor m' + id).toLowerCase();
      out.push({
        group: 'Motors',
        title: 'View ' + MOTOR_FULL[i] + ' motor',
        keywords: kw + ' detailed telemetry chart analytics',
        href: prefix() + 'detailed/?motor=' + id
      });
      out.push({
        group: 'Motors',
        title: 'Bench test ' + MOTOR_FULL[i] + ' motor',
        keywords: kw + ' test bench drive slider ramp manual',
        href: prefix() + 'test/?motor=' + id
      });
      out.push({
        group: 'Actions',
        title: 'Ramp test ' + MOTOR_FULL[i],
        keywords: kw + ' ramp cycle fwd rev automatic run',
        href: prefix() + 'test/?motor=' + id
      });
    });
    return out;
  }

  function actionEntries() {
    var p = prefix();
    return [
      {
        group: 'Actions',
        title: 'Start calibration sweep',
        keywords: 'calibration start sweep baseline begin run collect warmup',
        href: p + 'calibration/'
      },
      {
        group: 'Actions',
        title: 'Manual motor control',
        keywords: 'manual control drive slider motors calibration sliders',
        href: p + 'calibration/'
      }
    ];
  }

  var ALL = null;
  function allEntries() {
    if (!ALL) ALL = pageEntries().concat(motorEntries(), actionEntries());
    return ALL;
  }

  // Every whitespace-separated token must match title or keywords.
  function matchAll(q) {
    var tokens = q.toLowerCase().split(/\s+/).filter(function (t) { return t; });
    if (!tokens.length) return allEntries().slice();
    return allEntries().filter(function (e) {
      var hay = (e.title + ' ' + e.keywords).toLowerCase();
      return tokens.every(function (t) { return hay.indexOf(t) >= 0; });
    });
  }

  var backdrop = null, input = null, list = null;
  var rows = [];
  var selected = 0;

  function esc(s) {
    return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;')
      .replace(/>/g, '&gt;').replace(/"/g, '&quot;');
  }

  function build() {
    backdrop = document.createElement('div');
    backdrop.className = 'cmdk-backdrop';
    backdrop.id = 'cmdk-backdrop';
    backdrop.innerHTML =
      '<div class="cmdk-panel" role="dialog" aria-label="Search dashboard">' +
      '<div class="cmdk-input-row">' +
      '<svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.34-4.34"/></svg>' +
      '<input id="cmdk-input" type="text" placeholder="Search pages, motors, actions..." autocomplete="off" spellcheck="false">' +
      '<kbd>esc</kbd>' +
      '</div>' +
      '<div class="cmdk-list" id="cmdk-list"></div>' +
      '<div class="cmdk-foot"><span><kbd>&#8593;&#8595;</kbd> navigate</span><span><kbd>&#8629;</kbd> open</span><span><kbd>esc</kbd> close</span></div>' +
      '</div>';
    document.body.appendChild(backdrop);
    input = backdrop.querySelector('#cmdk-input');
    list = backdrop.querySelector('#cmdk-list');

    input.addEventListener('input', function () { render(input.value); });
    input.addEventListener('keydown', function (e) {
      if (e.key === 'ArrowDown') { e.preventDefault(); move(1); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); move(-1); }
      else if (e.key === 'Enter') { go(rows[selected]); }
      else if (e.key === 'Escape') { close(); }
    });
    backdrop.addEventListener('click', function (e) {
      if (e.target === backdrop) close();
    });
    list.addEventListener('click', function (e) {
      var row = e.target.closest ? e.target.closest('.cmdk-row') : null;
      if (row) go(row.getAttribute('data-href'));
    });
    list.addEventListener('mousemove', function (e) {
      var row = e.target.closest ? e.target.closest('.cmdk-row') : null;
      if (!row) return;
      var idx = rows.indexOf(row.getAttribute('data-href'));
      if (idx >= 0) select(idx);
    });
  }

  function render(q) {
    var hits = matchAll(q || '');
    rows = [];
    if (!hits.length) {
      list.innerHTML = '<div class="cmdk-empty">No results for &ldquo;' + esc(q || '') + '&rdquo;</div>';
      return;
    }
    var html = '', lastGroup = null;
    hits.forEach(function (e) {
      if (e.group !== lastGroup) {
        html += '<div class="cmdk-group">' + esc(e.group) + '</div>';
        lastGroup = e.group;
      }
      rows.push(e.href);
      html += '<button type="button" class="cmdk-row" data-href="' + esc(e.href) + '">' +
        '<span>' + esc(e.title) + '</span></button>';
    });
    list.innerHTML = html;
    select(0);
  }

  function select(i) {
    var els = list.querySelectorAll('.cmdk-row');
    if (i < 0) i = els.length - 1;
    if (i >= els.length) i = 0;
    selected = i;
    els.forEach(function (el, j) { el.classList.toggle('selected', j === i); });
    if (els[i] && els[i].scrollIntoView) {
      els[i].scrollIntoView({ block: 'nearest' });
    }
  }

  function move(d) { select(selected + d); }

  function go(href) {
    if (href) window.location.href = href;
  }

  function open() {
    if (!backdrop) build();
    backdrop.classList.add('open');
    input.value = '';
    render('');
    if (input.focus) input.focus();
  }

  function close() {
    if (!backdrop) return;
    backdrop.classList.remove('open');
    if (document.activeElement && document.activeElement.blur) {
      document.activeElement.blur();
    }
  }

  function isOpen() {
    return !!(backdrop && backdrop.classList.contains('open'));
  }

  document.querySelectorAll('.search-bar').forEach(function (bar) {
    bar.setAttribute('role', 'button');
    bar.setAttribute('tabindex', '0');
    bar.setAttribute('aria-label', 'Search dashboard (Ctrl+K)');
    bar.addEventListener('click', open);
    bar.addEventListener('keydown', function (e) {
      if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); open(); }
    });
  });

  document.addEventListener('keydown', function (e) {
    var mod = e.ctrlKey || e.metaKey;
    if (mod && (e.key === 'k' || e.key === 'K')) {
      e.preventDefault();
      if (isOpen()) close();
      else open();
    } else if (e.key === 'Escape' && isOpen()) {
      close();
    }
  });
})();

// ===== Native window controls (Tauri frameless shell only) =====
// The main window runs decorations:false, so min/max/close live in the
// header's actions slot. Rendered only when window.__TAURI__ exists —
// plain browsers never see these. Close keeps the tray behavior (hide,
// quit from the tray menu) because CloseRequested is still prevent_close.
(function() {
  'use strict';
  if (!window.__TAURI__ || !window.__TAURI__.window) return;
  var slots = document.querySelectorAll('.header-actions');
  if (!slots.length) return;
  var win = window.__TAURI__.window.getCurrentWindow();
  if (!win) return;

  function glyph(path) {
    return '<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" ' +
      'viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2" ' +
      'stroke-linecap="round" stroke-linejoin="round">' + path + '</svg>';
  }
  var SVG_MIN = glyph('<path d="M1.5 6h9"/>');
  var SVG_MAX = glyph('<rect x="2.5" y="2.5" width="7" height="7"/>');
  var SVG_RESTORE = glyph('<path d="M4.5 1.5h6v6M1.5 4.5v6h6"/>');
  var SVG_CLOSE = glyph('<path d="M2 2l8 8M10 2l-8 8"/>');

  // Every bridge call funnels through here: a missing method or a denied
  // permission degrades to hidden controls plus one console warning — never
  // to rendered-but-dead buttons.
  function hideControls(reason) {
    document.querySelectorAll('.win-controls').forEach(function (bar) {
      bar.style.display = 'none';
    });
    if (window.console && console.warn) {
      console.warn('[win-controls] disabled: ' + reason);
    }
  }

  function bridge(name, after) {
    var fn = win[name];
    if (typeof fn !== 'function') { hideControls('no method: ' + name); return; }
    var out;
    try { out = fn.call(win); } catch (e) { hideControls('call failed: ' + name); return; }
    if (out && typeof out.then === 'function') {
      out.then(function (v) { if (after) after(v); },
        function () { hideControls('denied: ' + name); });
    } else if (after) { after(out); }
  }

  function syncMax() {
    bridge('isMaximized', function (m) {
      document.querySelectorAll('.win-btn[data-win="max"]').forEach(function (b) {
        b.innerHTML = m ? SVG_RESTORE : SVG_MAX;
        b.setAttribute('title', m ? 'Restore' : 'Maximize');
        b.setAttribute('aria-label', m ? 'Restore' : 'Maximize');
      });
    });
  }

  slots.forEach(function (slot) {
    var bar = document.createElement('div');
    bar.className = 'win-controls';
    bar.innerHTML =
      '<button type="button" class="win-btn" data-win="min" title="Minimize" aria-label="Minimize">' + SVG_MIN + '</button>' +
      '<button type="button" class="win-btn" data-win="max" title="Maximize" aria-label="Maximize">' + SVG_MAX + '</button>' +
      '<button type="button" class="win-btn win-close" data-win="close" title="Close" aria-label="Close">' + SVG_CLOSE + '</button>';
    slot.appendChild(bar);
  });

  document.querySelectorAll('.win-btn').forEach(function (b) {
    b.addEventListener('click', function (e) {
      e.stopPropagation();
      var k = b.getAttribute('data-win');
      if (k === 'min') { bridge('minimize'); }
      else if (k === 'max') { bridge('toggleMaximize', syncMax); }
      else if (k === 'close') { bridge('close'); }
    });
  });

  // Drag the frameless window from the header (single driver, so no
  // attribute path exists to conflict with). Interactive elements opt out.
  // Double-click (not on controls) toggles maximize.
  var header = document.getElementById('site-header');
  if (header) {
    header.addEventListener('mousedown', function (e) {
      if (e.button !== 0) return;
      if (e.target.closest && e.target.closest('button, a, input, .search-bar, .win-controls')) return;
      bridge('startDragging');
    });
    header.addEventListener('dblclick', function (e) {
      if (e.target.closest && e.target.closest('button, a, input, .search-bar')) return;
      bridge('toggleMaximize', syncMax);
    });
  }

  // Keep the glyph honest when maximized externally (Win+arrows, taskbar).
  var lastSync = 0;
  if (typeof win.onResized === 'function') {
    try {
      var sub = win.onResized(function () {
        var now = Date.now();
        if (now - lastSync > 250) { lastSync = now; syncMax(); }
      });
      if (sub && typeof sub.catch === 'function') {
        sub.catch(function () { hideControls('denied: onResized'); });
      }
    } catch (e) { hideControls('call failed: onResized'); }
  }
  syncMax();
})();
