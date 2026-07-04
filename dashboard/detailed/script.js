(function() {
  'use strict';

  var motorData = {
    1: { label: 'FL', temp: 38, voltage: 12.3, current: 0.6, health: 92 },
    2: { label: 'FR', temp: 42, voltage: 12.1, current: 0.8, health: 87 },
    3: { label: 'RL', temp: 35, voltage: 12.4, current: 0.5, health: 95 },
    4: { label: 'RR', temp: 48, voltage: 11.8, current: 1.2, health: 78 }
  };

  var history = { 1: [], 2: [], 3: [], 4: [] };
  var maxPoints = 60;
  var currentMotor = 1;

  // Canvas references
  var canvases = {
    temp: document.getElementById('chart-temp'),
    voltage: document.getElementById('chart-voltage'),
    current: document.getElementById('chart-current'),
    health: document.getElementById('chart-health')
  };

  function getCtx(id) { return canvases[id] ? canvases[id].getContext('2d') : null; }

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
  function drawChart(canvasId, data, color, label, unit) {
    var ctx = getCtx(canvasId);
    if (!ctx) return;
    var w = ctx.canvas.width;
    var h = ctx.canvas.height;
    var pad = { top: 12, bottom: 16, left: 8, right: 8 };
    var plotW = w - pad.left - pad.right;
    var plotH = h - pad.top - pad.bottom;

    ctx.clearRect(0, 0, w, h);

    // Background
    ctx.fillStyle = 'transparent';

    if (data.length < 2) {
      ctx.fillStyle = 'rgba(255,255,255,0.08)';
      ctx.font = '11px Inter, sans-serif';
      ctx.textAlign = 'center';
      ctx.fillText('Waiting for data...', w / 2, h / 2 + 4);
      return;
    }

    var min = Math.min.apply(null, data);
    var max = Math.max.apply(null, data);
    var range = max - min || 1;
    var padding = range * 0.15;
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
      var yVal = pad.top + plotH - ((data[j] - min) / range) * plotH;
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
      var y2 = pad.top + plotH - ((data[k] - min) / range) * plotH;
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
    var cards = [
      { label: 'Temperature', value: m.temp.toFixed(1), unit: '°C', badge: 'normal', badgeClass: 'dot-green' },
      { label: 'Voltage', value: m.voltage.toFixed(2), unit: 'V', badge: m.voltage >= 12 ? 'stable' : 'low', badgeClass: m.voltage >= 12 ? 'dot-green' : 'dot-yellow' },
      { label: 'Current', value: m.current.toFixed(3), unit: 'A', badge: m.current < 1.5 ? 'nominal' : 'high', badgeClass: m.current < 1.5 ? 'dot-green' : 'dot-yellow' },
      { label: 'Health', value: Math.round(m.health), unit: '%', badge: m.health > 70 ? 'good' : (m.health > 40 ? 'fair' : 'critical'), badgeClass: m.health > 70 ? 'dot-green' : (m.health > 40 ? 'dot-yellow' : 'dot-red') }
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

  // ===== Simulate data =====
  function tick() {
    var m = motorData[currentMotor];
    m.temp += (Math.random() - 0.5) * 1.2;
    m.temp = Math.max(30, Math.min(65, m.temp));
    m.voltage += (Math.random() - 0.5) * 0.1;
    m.voltage = Math.max(10.5, Math.min(14, m.voltage));
    m.current += (Math.random() - 0.5) * 0.08;
    m.current = Math.max(0, Math.min(3, m.current));
    m.health += (Math.random() - 0.5) * 0.5;
    m.health = Math.max(10, Math.min(100, m.health));

    var hist = history[currentMotor];
    hist.push(m.temp);
    if (hist.length > maxPoints) hist.shift();

    renderValueCards();
    drawChart('chart-temp', hist, '#f97316', 'Temp', '°C');
    drawChart('chart-voltage', history[currentMotor].slice().map(function(v, i) { return motorData[currentMotor].voltage + (Math.random()-0.5)*0.1; }), '#22c55e', 'Voltage', 'V');
    drawChart('chart-current', history[currentMotor].slice().map(function(v, i) { return motorData[currentMotor].current + (Math.random()-0.5)*0.05; }), '#3b82f6', 'Current', 'A');
    drawChart('chart-health', history[currentMotor].slice().map(function(v) { return motorData[currentMotor].health; }), '#a855f7', 'Health', '%');
  }

  // ===== Motor tab switching =====
  var tabs = document.querySelectorAll('.detail-motor-tab');
  tabs.forEach(function(tab) {
    tab.addEventListener('click', function() {
      tabs.forEach(function(t) { t.classList.remove('active'); });
      this.classList.add('active');
      currentMotor = parseInt(this.getAttribute('data-motor'), 10);
      if (!history[currentMotor]) history[currentMotor] = [];
      renderValueCards();
      tick();
    });
  });

  // ===== Init =====
  window.addEventListener('resize', function() {
    resizeCanvases();
    tick();
  });
  resizeCanvases();
  renderValueCards();
  updateTimestamp();
  setInterval(function() { tick(); updateTimestamp(); }, 800);
})();
