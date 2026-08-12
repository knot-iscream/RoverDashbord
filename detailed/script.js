(function() {
  'use strict';

  // Auto-detect backend server from page location (fixes multi-PC access)
  var WS_URL = (window.location.protocol === 'https:' ? 'wss:' : 'ws:') + 
               '//' + window.location.hostname + ':8000/ws';
  var DETAIL_API = (window.location.protocol === 'https:' ? 'https:' : 'http:') + 
                   '//' + window.location.hostname + ':8000';
  var maxPoints = 60;
  var currentMotor = 1;

  // Live telemetry per motor (real ESP32 samples only — no simulation).
  var motorData = {
    1: { label: 'FL', temp: null, voltage: null, current: null, health: null,
         temp_valid: true, ina_ok: true },
    2: { label: 'FR', temp: null, voltage: null, current: null, health: null,
         temp_valid: true, ina_ok: true },
    3: { label: 'RL', temp: null, voltage: null, current: null, health: null,
         temp_valid: true, ina_ok: true },
    4: { label: 'RR', temp: null, voltage: null, current: null, health: null,
         temp_valid: true, ina_ok: true }
  };

  var history = { 1: [], 2: [], 3: [], 4: [] };
  var deviceOnline = false;

  // Data freshness — only real motor_update packets count as "live".
  var lastRxAt = { 1: null, 2: null, 3: null, 4: null };
  var FRESH_LIVE_MS = 5000;     // live while samples arrive within this window
  var FRESH_TIMEOUT_MS = 60000; // after this, drop back to placeholders
  var freshTimer = null;

  // Canvas references
  var canvases = {
    temp: document.getElementById('chart-temp'),
    voltage: document.getElementById('chart-voltage'),
    current: document.getElementById('chart-current'),
    health: document.getElementById('chart-health')
  };

  function getCtx(id) { return canvases[id] ? canvases[id].getContext('2d') : null; }

  function isReal(v) { return typeof v === 'number' && isFinite(v); }

  // ===== Resize canvases =====
  function resizeCanvases() {
    Object.keys(canvases).forEach(function(key) {
      var c = canvases[key];
      if (c) {
        var rect = c.parentElement.getBoundingClientRect();
        c.width = rect.width || 200;
        c.height = 140;
      }
    });
  }

  // ===== Draw a line chart on a canvas =====
  // `range` pins the scale when given, otherwise it auto-fits the data.
  function drawChart(canvasId, data, color, label, unit, range) {
    var ctx = getCtx(canvasId);
    if (!ctx) return;
    var w = ctx.canvas.width;
    var h = ctx.canvas.height;
    var pad = { top: 12, bottom: 16, left: 8, right: 8 };
    var plotW = w - pad.left - pad.right;
    var plotH = h - pad.top - pad.bottom;

    ctx.clearRect(0, 0, w, h);

    if (data.length < 2) {
      ctx.fillStyle = 'rgba(255,255,255,0.08)';
      ctx.font = '11px Inter, sans-serif';
      ctx.textAlign = 'center';
      ctx.fillText('Waiting for live data...', w / 2, h / 2 + 4);
      return;
    }

    var min = Math.min.apply(null, data);
    var max = Math.max.apply(null, data);
    if (range) {
      min = range[0];
      max = range[1];
    }
    var span = max - min || 1;
    var padding = span * 0.15;
    min -= padding;
    max += padding;

    // Grid lines
    ctx.strokeStyle = 'rgba(255,255,255,0.04)';
    ctx.lineWidth = 1;
    for (var i = 0; i <= 4; i++) {
      var y = pad.top + (plotH / 4) * i;
      ctx.beginPath();
      ctx.moveTo(pad.left, y);
      ctx.lineTo(w - pad.right, y);
      ctx.stroke();
    }

    // Data line
    ctx.strokeStyle = color;
    ctx.lineWidth = 2;
    ctx.lineJoin = 'round';
    ctx.lineCap = 'round';
    ctx.beginPath();
    for (var j = 0; j < data.length; j++) {
      var x = pad.left + (j / (data.length - 1)) * plotW;
      var yVal = pad.top + plotH - ((data[j] - min) / (max - min)) * plotH;
      if (j === 0) ctx.moveTo(x, yVal);
      else ctx.lineTo(x, yVal);
    }
    ctx.stroke();

    // Glow
    ctx.strokeStyle = color.replace(')', ',0.15)').replace('rgb', 'rgba');
    ctx.lineWidth = 6;
    ctx.beginPath();
    for (var k = 0; k < data.length; k++) {
      var x2 = pad.left + (k / (data.length - 1)) * plotW;
      var y2 = pad.top + plotH - ((data[k] - min) / (max - min)) * plotH;
      if (k === 0) ctx.moveTo(x2, y2);
      else ctx.lineTo(x2, y2);
    }
    ctx.stroke();

    // Latest value
    var last = data[data.length - 1];
    ctx.fillStyle = color;
    ctx.font = '13px Inter, sans-serif';
    ctx.fontWeight = '700';
    ctx.textAlign = 'right';
    ctx.fillText(last.toFixed(1), w - pad.right, pad.top + 12);
    ctx.fillStyle = 'rgba(255,255,255,0.3)';
    ctx.font = '9px Inter, sans-serif';
    ctx.fillText(unit, w - pad.right, pad.top + 24);
  }

  // ===== Update value cards =====
  function updateTimestamp() {
    var el = document.getElementById('update-time');
    if (el) el.textContent = new Date().toLocaleTimeString();
  }

  function renderValueCards() {
    var m = motorData[currentMotor];
    var container = document.getElementById('value-cards');

    var stale = isStale(currentMotor);
    var tempTxt = (!stale && m.temp_valid && isReal(m.temp))
      ? m.temp.toFixed(1) : '--';
    var voltTxt = (!stale && m.ina_ok && isReal(m.voltage))
      ? m.voltage.toFixed(2) : '--';
    var currTxt = (!stale && m.ina_ok && isReal(m.current))
      ? m.current.toFixed(3) : '--';
    var healthTxt = (!stale && isReal(m.health)) ? Math.round(m.health) : '--';

    var cards = [
      { label: 'Temperature', value: tempTxt, unit: '°C', badge: (!stale && m.temp_valid) ? 'normal' : 'no probe', badgeClass: (!stale && m.temp_valid) ? 'dot-green' : 'dot-red' },
      { label: 'Voltage', value: voltTxt, unit: 'V', badge: (!stale && m.ina_ok) ? (m.voltage >= 12 ? 'stable' : 'low') : 'no chip', badgeClass: (!stale && m.ina_ok) ? (m.voltage >= 12 ? 'dot-green' : 'dot-yellow') : 'dot-red' },
      { label: 'Current', value: currTxt, unit: 'A', badge: (!stale && m.ina_ok) ? (m.current < 1.5 ? 'nominal' : 'high') : 'no chip', badgeClass: (!stale && m.ina_ok) ? (m.current < 1.5 ? 'dot-green' : 'dot-yellow') : 'dot-red' },
      { label: 'Health', value: healthTxt, unit: '%', badge: (!stale && isReal(m.health)) ? (m.health > 70 ? 'good' : (m.health > 40 ? 'fair' : 'critical')) : 'no data', badgeClass: (!stale && isReal(m.health)) ? (m.health > 70 ? 'dot-green' : (m.health > 40 ? 'dot-yellow' : 'dot-red')) : 'dot-red' }
    ];
    var html = '';
    cards.forEach(function(c) {
      html += '<div class="value-card">';
      html += '<div class="value-label">' + c.label + '</div>';
      html += '<div class="value-number">' + c.value + '<span class="value-unit">' + c.unit + '</span></div>';
      html += '<span class="value-badge" style="display:inline-flex;align-items:center;gap:4px;"><span class="status-dot ' + c.badgeClass + '" style="display:inline-block;"></span>' + c.badge + '</span>';
      html += '</div>';
    });
    container.innerHTML = html;
  }

  // ===== Telemetry ingestion (live WS only) =====
  function applySample(msg) {
    var id = msg.motor;
    if (!motorData[id]) return;
    var m = motorData[id];
    m.temp_valid = msg.temp_valid !== false;
    m.ina_ok = msg.ina_ok !== false;
    lastRxAt[id] = performance.now();

    if (m.temp_valid && isReal(msg.temp)) {
      m.temp = msg.temp;
      pushOne('temp', id, msg.temp);
    } else {
      m.temp = null;
    }
    if (m.ina_ok && isReal(msg.voltage)) {
      m.voltage = msg.voltage;
      pushOne('voltage', id, msg.voltage);
    } else {
      m.voltage = null;
    }
    if (m.ina_ok && isReal(msg.current)) {
      m.current = msg.current;
      pushOne('current', id, msg.current);
    } else {
      m.current = null;
    }
    if (typeof msg.health === 'number') {
      m.health = msg.health;
      pushOne('health', id, msg.health);
    } else {
      m.health = null;
    }
    renderCurrent();
  }

  function pushOne(key, id, val) {
    var hist = history[id];
    if (!hist) history[id] = hist = [];
    hist.push(val);
    if (hist.length > maxPoints) hist.shift();
    if (id === currentMotor) {
      if (key === 'temp') drawChart('chart-temp', history[currentMotor], '#f97316', 'Temp', '°C', [20, 80]);
      else if (key === 'voltage') drawChart('chart-voltage', history[currentMotor], '#22c55e', 'Voltage', 'V', [10, 14]);
      else if (key === 'current') drawChart('chart-current', history[currentMotor], '#3b82f6', 'Current', 'A', [0, 3]);
      else if (key === 'health') drawChart('chart-health', history[currentMotor], '#a855f7', 'Health', '%', [0, 100]);
    }
  }

  function renderCurrent() {
    renderValueCards();
    var stale = isStale(currentMotor);
    var data = stale ? [] : (history[currentMotor] || []);
    drawChart('chart-temp', data, '#f97316', 'Temp', '°C', [20, 80]);
    drawChart('chart-voltage', data, '#22c55e', 'Voltage', 'V', [10, 14]);
    drawChart('chart-current', data, '#3b82f6', 'Current', 'A', [0, 3]);
    drawChart('chart-health', data, '#a855f7', 'Health', '%', [0, 100]);
  }

  // ===== Data freshness (real vs stale vs nothing) =====
  function isStale(id) {
    var last = lastRxAt[id];
    if (last === null) return true;
    var age = performance.now() - last;
    return age > FRESH_LIVE_MS;
  }

  function updateFreshness() {
    var changed = false;
    [1, 2, 3, 4].forEach(function (id) {
      var before = isStale(id);
      var after = (lastRxAt[id] !== null) &&
        (performance.now() - lastRxAt[id]) > FRESH_LIVE_MS;
      if (before !== after) changed = true;
    });
    if (changed) renderCurrent();
  }

  // ===== WebSocket (real backing data) =====
  var ws = null;
  function connectWS() {
    if (ws && ws.readyState === WebSocket.OPEN) return;
    ws = new WebSocket(WS_URL);
    ws.onopen = function () { ws.send('ping'); };
    ws.onmessage = function (e) {
      try {
        var msg = JSON.parse(e.data);
        if (msg.type === 'motor_update') {
          applySample(msg);
        } else if (msg.type === 'user_joined' || msg.type === 'user_left' || msg.type === 'user_presence') {
          // Pass user presence messages through to global script.js
          // (handled by window.dashboardWS which is the same connection)
        } else if (msg.type === 'snapshot') {
          // Cached history replay — intentionally ignored. Only fresh
          // motor_update packets (real hardware samples) drive this page,
          // so an unplugged/disconnected sensor never leaves stale or
          // fake-looking values on screen.
        } else if (msg.type === 'device_status') {
          applyDeviceStatus(msg.online);
        }
      } catch (err) { /* ignore */ }
    };
    ws.onclose = function () {
      ws = null;
      setTimeout(connectWS, 3000);
    };
    ws.onerror = function () { ws.close(); };
  }

  // ===== Motor tab switching =====
  var tabs = document.querySelectorAll('.detail-motor-tab');
  tabs.forEach(function(tab) {
    tab.addEventListener('click', function() {
      tabs.forEach(function(t) { t.classList.remove('active'); });
      this.classList.add('active');
      currentMotor = parseInt(this.getAttribute('data-motor'), 10);
      if (!history[currentMotor]) history[currentMotor] = [];
      renderCurrent();
    });
  });

  // ===== Device presence (real ESP32 heartbeat via backend) =====
  function applyDeviceStatus(online) {
    deviceOnline = online === true;
    var dot = document.getElementById('detail-live-dot');
    var label = document.getElementById('detail-live-label');
    if (dot) dot.className = deviceOnline ? 'status-dot dot-green pulse' : 'status-dot dot-red';
    if (label) label.textContent = deviceOnline ? 'Live' : 'Offline';
  }

  function pollDeviceStatus() {
    var xhr = new XMLHttpRequest();
    xhr.open('GET', DETAIL_API + '/api/device/status', true);
    xhr.onload = function () {
      var online = false;
      if (xhr.status === 200) {
        try { online = JSON.parse(xhr.responseText).online === true; } catch (e) { online = false; }
      }
      applyDeviceStatus(online);
    };
    xhr.onerror = function () { applyDeviceStatus(false); };
    xhr.send();
  }

  // ===== Init =====
  window.addEventListener('resize', function() {
    resizeCanvases();
    renderCurrent();
  });
  resizeCanvases();
  renderValueCards();
  updateTimestamp();
  pollDeviceStatus();
  setInterval(pollDeviceStatus, 3000);
  setInterval(updateTimestamp, 1000);
  freshTimer = setInterval(updateFreshness, 1000);
  connectWS();
  renderCurrent();
})();