// ===== Lucide icons =====
lucide.createIcons();

// ===== Scroll-based header border =====
(function() {
  const header = document.getElementById('site-header');
  let ticking = false;
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
})();

// ===== Mobile nav active state =====
document.querySelectorAll('.mobile-nav-btn').forEach(function(btn) {
  btn.addEventListener('click', function() {
    document.querySelectorAll('.mobile-nav-btn').forEach(function(b) {
      b.classList.remove('active');
    });
    this.classList.add('active');
  });
});

// ===== Sidebar nav active state =====
document.querySelectorAll('.nav-item').forEach(function(item) {
  item.addEventListener('click', function() {
    document.querySelectorAll('.nav-item').forEach(function(n) {
      n.classList.remove('active');
    });
    this.classList.add('active');
  });
});

// ===== Trending tabs switcher =====
(function() {
  const tabs = document.querySelectorAll('.trending-tab');
  const items = document.querySelectorAll('.trending-item');
  if (!tabs.length || !items.length) return;
  function switchTab(tabName) {
    tabs.forEach(function(t) {
      t.classList.remove('active');
      if (t.getAttribute('data-tab') === tabName) {
        t.classList.add('active');
      }
    });
    items.forEach(function(item) {
      if (item.getAttribute('data-tab-group') === tabName) {
        item.classList.remove('hidden');
      } else {
        item.classList.add('hidden');
      }
    });
  }
  tabs.forEach(function(tab) {
    tab.addEventListener('click', function() {
      switchTab(this.getAttribute('data-tab'));
    });
  });
  switchTab('day');
})();
