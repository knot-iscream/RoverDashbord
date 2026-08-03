(function() {
  'use strict';

  var motors = [
    { id: 1, label: 'FL', health: 92, temp: 38, vibration: 0, voltage: 12.3, current: 0.6 },
    { id: 2, label: 'FR', health: 87, temp: 42, vibration: 0, voltage: 12.1, current: 0.8 },
    { id: 3, label: 'RL', health: 95, temp: 35, vibration: 0, voltage: 12.4, current: 0.5 },
    { id: 4, label: 'RR', health: 78, temp: 48, vibration: 1, voltage: 11.8, current: 1.2 }
  ];

  var wheelSpecs = {
    wheelFL: { cx: 237,     fullW: 49.58 },
    wheelFR: { cx: 361.272, fullW: 17.76 },
    wheelRL: { cx: 237,     fullW: 28.86 },
    wheelRR: { cx: 361.272, fullW: 37    }
  };

  var damageTexts = [];
  var textTimer = null;

  function tempColor(temp) {
    var t = Math.max(20, Math.min(60, temp));
    var pct = (t - 20) / 40;
    var hue = (1 - pct) * 240;
    return 'hsl(' + Math.round(hue) + ', 100%, 50%)';
  }

  function updatePart(partId, healthPct, temp) {
    var el = document.querySelector('[data-part="' + partId + '"]');
    if (!el) return;

    var color = tempColor(temp);

    var spec = wheelSpecs[partId];
    if (spec) {
      var fillW = spec.fullW * healthPct / 100;
      el.setAttribute('x', spec.cx + spec.fullW - fillW);
      el.setAttribute('width', fillW);
    }

    if (partId === 'chassis') {
      var totalH = parseFloat(el.getAttribute('data-total-height')) || 419;
      var newH = totalH * healthPct / 100;
      el.setAttribute('y', 61 + totalH - newH);
      el.setAttribute('height', newH);
    }

    el.setAttribute('fill', color);
    var prevTemp = parseFloat(el.getAttribute('data-prev-temp')) || 0;
    if (Math.abs(temp - prevTemp) > 3) {
      el.classList.remove('dmg-flash');
      void el.offsetWidth;
      el.classList.add('dmg-flash');
    }
    el.setAttribute('data-prev-temp', temp);
  }

  function getAvgHealth() {
    var sum = 0;
    motors.forEach(function(m) { sum += m.health; });
    return sum / motors.length;
  }

  function getAvgTemp() {
    var sum = 0;
    motors.forEach(function(m) { sum += m.temp; });
    return sum / motors.length;
  }

  function getMinVoltage() {
    var min = 99;
    motors.forEach(function(m) { if (m.voltage < min) min = m.voltage; });
    return min;
  }

  function updatePanelStatus() {
    var dot = document.getElementById('dmg-status-dot');
    var text = document.getElementById('dmg-status-text');
    if (!dot || !text) return;
    var worst = 0;
    motors.forEach(function(m) {
      if (m.health < 50 || m.temp > 50 || m.vibration) worst = 2;
      else if (worst < 1 && (m.health < 80 || m.temp > 40)) worst = 1;
    });
    if (getMinVoltage() < 11) worst = 2;
    else if (worst < 1 && getMinVoltage() < 12) worst = 1;
    if (worst === 0) {
      dot.className = 'status-dot dot-green pulse';
      text.textContent = 'All systems nominal';
    } else if (worst === 1) {
      dot.className = 'status-dot dot-yellow pulse';
      text.textContent = 'Service advised';
    } else {
      dot.className = 'status-dot dot-red pulse';
      text.textContent = 'Damage detected';
    }
  }

  function updateDamageQueue() {
    damageTexts = [];
    motors.forEach(function(m) {
      if (m.health < 50) {
        damageTexts.push(m.label + ' motor critical (' + Math.round(m.health) + '%)');
      } else if (m.health < 80) {
        damageTexts.push(m.label + ' motor worn (' + Math.round(m.health) + '%)');
      }
      if (m.temp > 50) {
        damageTexts.push(m.label + ' overheating (' + Math.round(m.temp) + '\u00B0C)');
      }
      if (m.vibration) {
        damageTexts.push(m.label + ' abnormal vibration');
      }
    });
    var minVolt = getMinVoltage();
    if (minVolt < 11) {
      damageTexts.push('Battery critical (' + minVolt.toFixed(1) + 'V)');
    } else if (minVolt < 12) {
      damageTexts.push('Battery low (' + minVolt.toFixed(1) + 'V)');
    }
    if (damageTexts.length === 0) {
      damageTexts.push('All systems nominal');
    }
    updatePanelStatus();
  }

  function cycleText() {
    if (damageTexts.length === 0) {
      damageTexts = ['All systems nominal'];
    }
    var txt = damageTexts.shift();
    damageTexts.push(txt);
    var overlay = document.getElementById('dmgOverlay');
    var textEl = document.getElementById('dmgText');
    if (textEl) textEl.textContent = txt;
    if (overlay) overlay.setAttribute('opacity', '1');
    clearTimeout(textTimer);
    textTimer = setTimeout(cycleText, 2000);
  }

  function updateCornerPanels() {
    motors.forEach(function(m) {
      var tempEl = document.getElementById('temp-' + m.label.toLowerCase());
      var healthEl = document.getElementById('health-' + m.label.toLowerCase());
      var color = tempColor(m.temp);
      if (tempEl) {
        tempEl.textContent = Math.round(m.temp) + '\u00B0C';
        tempEl.setAttribute('fill', color);
      }
      if (healthEl) {
        healthEl.textContent = Math.round(m.health) + '%';
        healthEl.setAttribute('fill', color);
      }
    });
  }

  function updateHUD() {
    var avgHealth = getAvgHealth();
    var avgTemp = getAvgTemp();
    updatePart('chassis', avgHealth, avgTemp);
    motors.forEach(function(m) {
      updatePart('wheel' + m.label, m.health, m.temp);
    });
    updateCornerPanels();
    updateDamageQueue();
    if (!textTimer) cycleText();
  }

  function simulateData() {
    motors.forEach(function(m) {
      m.health += (Math.random() - 0.5) * 0.8;
      m.health = Math.max(10, Math.min(100, m.health));
      m.temp += (Math.random() - 0.5) * 1.5;
      m.temp = Math.max(30, Math.min(65, m.temp));
      m.voltage += (Math.random() - 0.5) * 0.15;
      m.voltage = Math.max(10.5, Math.min(14, m.voltage));
      m.current += (Math.random() - 0.5) * 0.1;
      m.current = Math.max(0, Math.min(3, m.current));
      if (Math.random() < 0.02) m.vibration = 1;
      if (Math.random() < 0.01) m.vibration = 0;
      if (m.health < 50 && Math.random() < 0.03) m.vibration = 1;
    });
    updateHUD();
  }

  var camTabs = document.querySelectorAll('#cam-tabs .tab-btn');
  var camViewport = document.getElementById('camera-viewport');
  var camPlaceholder = document.getElementById('cam-placeholder');
  var camLabel = document.getElementById('cam-label');

  camTabs.forEach(function(tab) {
    tab.addEventListener('click', function() {
      camTabs.forEach(function(t) { t.classList.remove('active'); });
      this.classList.add('active');
      var mode = this.getAttribute('data-cam');
      if (mode === 'lidar') {
        camPlaceholder.style.opacity = '0.4';
        camViewport.style.background = '#050505';
        camLabel.textContent = 'LiDAR map stream \u2014 coming soon';
      } else {
        camPlaceholder.style.opacity = '1';
        camViewport.style.background = '#0a0a0a';
        camLabel.textContent = 'Waiting for camera stream...';
      }
    });
  });

  updateHUD();
  setInterval(simulateData, 600);

})();
