(function() {
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

  // ===== Edge strip: toggle pill dock visibility =====
  var dock = document.getElementById('nav-dock');
  var edgeToggle = document.getElementById('dock-edge-toggle');
  var edgeLock = document.getElementById('dock-edge-lock');
  var dockHidden = true;
  var dockLocked = localStorage.getItem('dock-locked') === 'true';

  function setDockVisible(visible) {
    if (!dock) return;
    dockHidden = !visible;
    dock.classList.toggle('dock-hidden', !visible);
    var edge = document.getElementById('dock-edge');
    if (edge) edge.classList.toggle('dock-edge-hidden', visible);
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

  // Apply initial locked state
  if (dockLocked) applyLockState();

  function onDocumentClick(e) {
    if (dockLocked) return;
    var edge = document.getElementById('dock-edge');
    if (dock && !dock.contains(e.target) && edge && !edge.contains(e.target)) {
      setDockVisible(false);
      document.removeEventListener('click', onDocumentClick);
    }
  }

  if (edgeToggle && dock) {
    edgeToggle.addEventListener('click', function() {
      if (dockLocked) return;
      var isHidden = dock.classList.contains('dock-hidden') || dock.classList.contains('dock-locked');
      setDockVisible(isHidden);
      var icon = edgeToggle.querySelector('svg');
      if (icon) {
        if (isHidden) {
          icon.innerHTML = '<path d="m15 18-6-6 6-6"/>';
          document.addEventListener('click', onDocumentClick);
        } else {
          icon.innerHTML = '<path d="m9 18 6-6-6-6"/>';
          document.removeEventListener('click', onDocumentClick);
        }
      }
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

  function updateIndicator() {
    if (!indicator) return;
    var active = document.querySelector('.dock-item.active');
    if (active) {
      indicator.style.transform = 'translateY(' + getItemOffset(active) + 'px)';
    }
  }

  dockItems.forEach(function(item) {
    item.addEventListener('click', function() {
      dockItems.forEach(function(n) { n.classList.remove('active'); });
      this.classList.add('active');
      updateIndicator();
    });
  });

  // ===== Highlight current page =====
  var path = window.location.pathname;
  dockItems.forEach(function(item) {
    var href = item.getAttribute('data-href');
    if (href && path.indexOf(href) !== -1) {
      dockItems.forEach(function(n) { n.classList.remove('active'); });
      item.classList.add('active');
    }
  });
  updateIndicator();

  // ===== Mobile bottom nav active =====
  var mobileBtns = document.querySelectorAll('.mobile-nav-btn');
  mobileBtns.forEach(function(btn) {
    btn.addEventListener('click', function() {
      mobileBtns.forEach(function(b) { b.classList.remove('active'); });
      this.classList.add('active');
    });
  });
  mobileBtns.forEach(function(btn) {
    var href = btn.getAttribute('data-href');
    if (href && path.indexOf(href) !== -1) {
      mobileBtns.forEach(function(b) { b.classList.remove('active'); });
      btn.classList.add('active');
    }
  });
})();
