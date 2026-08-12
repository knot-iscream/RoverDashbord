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
      var href = this.getAttribute('data-href');
      if (href) window.location.href = href;
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
  updateIndicator();

  // ===== User Presence Badge & Notifications ═════════════════════════════
  
  var userPresenceBadge = null;
  var userBusyModal = null;
  var userCount = 1;  // Start with 1 (self)
  
  function createUserPresenceBadge() {
    if (userPresenceBadge) return userPresenceBadge;
    
    var badge = document.createElement('div');
    badge.className = 'user-presence-badge users-1';
    badge.innerHTML = '<div class="user-presence-dot"></div><span>1 user</span>';
    document.body.appendChild(badge);
    return badge;
  }
  
  function createUserBusyModal() {
    if (userBusyModal) return userBusyModal;
    
    var modal = document.createElement('div');
    modal.className = 'user-busy-modal';
    modal.innerHTML = `
      <div class="user-busy-card">
        <div class="user-busy-header">
          <div class="user-busy-icon">⚙</div>
          <div class="user-busy-title">Dashboard in Use</div>
        </div>
        <div class="user-busy-message">
          Another user is currently viewing the dashboard. Your changes will be synchronized in real-time. You can continue browsing, or wait for them to disconnect.
        </div>
        <div class="user-busy-actions">
          <button class="user-busy-btn user-busy-btn-cancel" onclick="window.location.href='/'">Return Home</button>
          <button class="user-busy-btn user-busy-btn-wait" onclick="this.closest('.user-busy-modal').classList.remove('show')">Continue</button>
        </div>
      </div>
    `;
    document.body.appendChild(modal);
    return modal;
  }
  
  function createNotificationToast(message, type) {
    var toast = document.createElement('div');
    toast.className = 'notification-toast ' + type;
    toast.textContent = message;
    document.body.appendChild(toast);
    
    setTimeout(function() {
      toast.style.opacity = '0';
      toast.style.transition = 'opacity 0.3s ease';
      setTimeout(function() { toast.remove(); }, 300);
    }, 3000);
  }
  
  function updateUserPresenceBadge(count) {
    userCount = count;
    var badge = userPresenceBadge || createUserPresenceBadge();
    var countText = count + ' user' + (count !== 1 ? 's' : '');
    badge.innerHTML = '<div class="user-presence-dot"></div><span>' + countText + '</span>';
    
    badge.className = 'user-presence-badge';
    if (count === 1) badge.classList.add('users-1');
    else if (count === 2) badge.classList.add('users-2');
    else badge.classList.add('users-2plus');
  }
  
  // Hook into global WebSocket for user presence
  if (typeof window.userPresenceInit === 'undefined') {
    window.userPresenceInit = true;
    
    // Wait for page WS to initialize, then hook into messages
    var presenceCheckInterval = setInterval(function() {
      // Try to find existing WS from calibration or app.js
      if (window.dashboardWS && window.dashboardWS.readyState === WebSocket.OPEN) {
        clearInterval(presenceCheckInterval);
        createUserPresenceBadge();
      }
    }, 100);
    
    // Also connect our own WS for user presence if pages don't have one
    if (!window.dashboardWS) {
      var wsUrl = (window.location.protocol === 'https:' ? 'wss:' : 'ws:') + 
                  '//' + window.location.hostname + ':8000/ws';
      try {
        var presenceWS = new WebSocket(wsUrl);
        window.dashboardWS = presenceWS;
        
        presenceWS.onopen = function() {
          console.log('[User Presence] Connected');
          createUserPresenceBadge();
        };
        
        presenceWS.onmessage = function(e) {
          try {
            var msg = JSON.parse(e.data);
            
            if (msg.type === 'user_joined') {
              updateUserPresenceBadge(msg.users_online);
              if (msg.users_online > 1) {
                createNotificationToast('User joined the dashboard', 'joined');
                // Show busy modal if this is the new user
                var modal = createUserBusyModal();
                modal.classList.add('show');
              }
            } else if (msg.type === 'user_left') {
              updateUserPresenceBadge(msg.users_online);
              if (msg.users_online >= 1) {
                createNotificationToast('User left the dashboard', 'left');
              }
            } else if (msg.type === 'user_presence') {
              updateUserPresenceBadge(msg.users_online);
            }
          } catch (e) {
            console.error('[User Presence] Message error:', e);
          }
        };
        
        presenceWS.onerror = function(err) {
          console.error('[User Presence] Error:', err);
        };
        
        presenceWS.onclose = function() {
          console.log('[User Presence] Disconnected');
        };
      } catch (e) {
        console.error('[User Presence] Connection error:', e);
      }
    }
  }

  // ===== Mobile bottom nav active =====
  var mobileBtns = document.querySelectorAll('.mobile-nav-btn');
  mobileBtns.forEach(function(btn) {
    btn.addEventListener('click', function() {
      mobileBtns.forEach(function(b) { b.classList.remove('active'); });
      this.classList.add('active');
      var href = this.getAttribute('data-href');
      if (href) window.location.href = href;
    });
  });
  mobileBtns.forEach(function(btn) {
    var href = btn.getAttribute('data-href');
    if (href) {
      var hrefPath = new URL(href, window.location.href).pathname;
      if (path.indexOf(hrefPath) !== -1) {
        mobileBtns.forEach(function(b) { b.classList.remove('active'); });
        btn.classList.add('active');
      }
    }
  });
})();
