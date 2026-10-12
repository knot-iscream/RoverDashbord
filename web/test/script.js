(function () {
  'use strict';

  // ── Config ────────────────────────────────────────────
  // Same backend that served this page — works from any PC/phone.
  var WS_URL = (window.ROVER && window.ROVER.wsUrl) || 'ws://localhost:8000/ws';
  var API_BASE = (window.ROVER && window.ROVER.apiBase) || 'http://localhost:8000';
  var MOTOR_NAMES = ['FL', 'FR', 'RL', 'RR'];
  var MOTOR_FULL = ['Front Left', 'Front Right', 'Rear Left', 'Rear Right'];
  var selectedMotor = 4;         // default RR (preserves previous behavior)

  // ── State ─────────────────────────────────────────────
  var ws = null;
  var calibrating = false;
  var rampRunning = false;
  var deviceOnline = false;
  var motorSpeeds = [0, 0, 0, 0];
  var motorAnim = null;
  var sendTimer = null;
  var pollTimer = null;
  var uiTimer = null;

  // Ramp sequence: FWD 25→50→75→100 → REV 100→75→50→25 → stop
  var RAMP_SEQ = [64, 128, 191, 255, -255, -191, -128, -64, 0];
  var RAMP_STEP_MS = 4000;
  var RAMP_GLIDE_MS = 650;
  var rampTimer = null;
  var rampIndex = 0;
  var rampStepStart = 0;

  // Data freshness — per-motor live samples only.
  var lastRxAt = { 1: null, 2: null, 3: null, 4: null }; // motorId -> performance.now()
  var lastSample = { 1: null, 2: null, 3: null, 4: null }; // motorId -> last payload
  var freshTimer = null;
  var FRESH_LIVE_MS = 5000;    // live while samples arrive within this window
  var FRESH_TIMEOUT_MS = 60000; // after this, drop back to placeholders

  // Health is only meaningful once the selected motor has a calibrated baseline.
  var hasBaseline = false;

  // ── DOM refs ──────────────────────────────────────────
  var statusDot = document.getElementById('test-status-dot');
  var statusLabel = document.getElementById('test-status-label');
  var teleDot = document.getElementById('tele-dot');
  var runStatus = document.getElementById('run-status');
  var sysStatus = document.getElementById('sys-status');
  var slider = document.getElementById('motor-slider');
  var valEl = document.getElementById('motor-val');
  var row = document.querySelector('[data-motor-row]');
  var offBtn = document.querySelector('[data-off]');
  var motorTitle = document.getElementById('test-motor-title');
  var motorName = document.getElementById('test-motor-name');
  var motorTabs = document.querySelectorAll('#test-motor-tabs .detail-motor-tab');
  var guard = document.getElementById('test-guard');
  var rampBtn = document.getElementById('ramp-btn');
  var rampStatus = document.getElementById('ramp-status');
  var presetBtns = document.querySelectorAll('.test-preset');
  var chipCal = document.getElementById('chip-cal');
  var chipCalLabel = document.getElementById('chip-cal-label');
  var chipConn = document.getElementById('chip-conn');
  var chipConnLabel = document.getElementById('chip-conn-label');
  var tAgeEl = document.getElementById('t-age');

  // ── Beat bars (vibration) ─────────────────────────────
  var vibBars = [];
  var vibRead = document.getElementById('vib-read');
  function buildBeats() {
    var wrap = document.getElementById('vib-bars');
    for (var i = 0; i < 20; i++) {
      var b = document.createElement('span');
      b.className = 'test-beat';
      b.style.setProperty('--h', (2 + Math.random() * 5).toFixed(1) + 'px');
      wrap.appendChild(b);
      vibBars.push(b);
    }
  }

  // ── WebSocket ─────────────────────────────────────────
  function connectWS() {
    if (ws && ws.readyState === WebSocket.OPEN) return;
    ws = new WebSocket(WS_URL);

    ws.onopen = function () {
      setConnectionStatus(true);
      ws.send('ping');
    };

    ws.onmessage = function (e) {
      try { handleWSMessage(JSON.parse(e.data)); } catch (err) { /* ignore */ }
    };

    ws.onclose = function () {
      setConnectionStatus(false);
      ws = null;
      setTimeout(connectWS, 3000);
    };

    ws.onerror = function () { ws.close(); };
  }

  function handleWSMessage(msg) {
    if (msg.type === 'motor_update') {
      if (msg.motor >= 1 && msg.motor <= 4) {
        lastRxAt[msg.motor] = performance.now();
        lastSample[msg.motor] = msg;
        if (msg.motor === selectedMotor) {
          renderTelemetry(msg);
          if (calibrating) renderRealStatus();
        }
      }
    } else if (msg.type === 'snapshot') {
      // Cached history replay — intentionally ignored. Only fresh
      // motor_update packets (real hardware samples) drive this page,
      // so a disconnected motor never leaves stale "fake-looking" data.
      updateFreshness();
    } else if (msg.type === 'calibration_status') {
      updateCalGuard(msg);
    } else if (msg.type === 'device_status') {
      applyDeviceStatus(msg.online);
    } else if (msg.type === 'pong') {
      // keep alive
    }
  }

  // Corner badge shows device presence only (Online/Offline). Server link
  // state lives in the guard chips below, telemetry freshness in the panel.
  function setConnectionStatus(connected) {
    chipConn.style.background = connected ? 'var(--dot-green)' : 'var(--dot-red)';
    chipConnLabel.textContent = connected ? 'Server connected' : 'Disconnected';
  }

  function applyDeviceStatus(online) {
    deviceOnline = online === true;
    statusDot.className = 'status-dot ' + (deviceOnline ? 'dot-green pulse' : 'dot-red');
    statusLabel.textContent = deviceOnline ? 'Online' : 'Offline';
    if (!deviceOnline) {
      runStatus.className = 'cal-motor-status idle';
      runStatus.textContent = 'Offline';
    } else if (!calibrating) {
      renderRealStatus();
    }
  }

  // ── Telemetry rendering ───────────────────────────────
  function fmt(x, dec, unit) {
    return (typeof x === 'number' && isFinite(x)) ? x.toFixed(dec) + unit : '--' + unit;
  }

  function renderTelemetry(d) {
    var tempOk = d.temp_valid !== false;
    document.getElementById('t-temp').textContent = tempOk
      ? fmt(d.temp, 1, ' \u00b0C') : '-- \u00b0C';

    var inaOk = d.ina_ok !== false;
    document.getElementById('t-volt').textContent = inaOk
      ? fmt(d.voltage, 2, ' V') : '-- V';
    document.getElementById('t-curr').textContent = inaOk
      ? fmt(d.current, 3, ' A') : '-- A';

    // Without a calibrated baseline there is no real health — show a dash.
    var h = (hasBaseline && typeof d.health === 'number') ? d.health : null;
    var hpct = h === null ? 0 : Math.min(100, Math.max(0, h));
    var hbar = document.getElementById('t-health-bar');
    hbar.style.width = hpct + '%';
    hbar.classList.toggle('low', h !== null && h < 60);
    document.getElementById('t-health').textContent =
      h === null ? '--' : h.toFixed(1) + '%';

    var spd = (typeof d.speed === 'number') ? d.speed : motorSpeeds[selectedMotor - 1];
    document.getElementById('t-speed').textContent = spd;

    renderVib(d.vibration, d.vibration_valid);
    renderSysStatus(d);
    if (!calibrating) {
      runStatus.className = 'cal-motor-status ' + (spd ? 'collecting' : 'idle');
      runStatus.textContent = spd ? 'Running' : 'Idle';
    }
  }

  function renderRealStatus() {
    var spd = motorSpeeds[selectedMotor - 1];
    runStatus.className = 'cal-motor-status ' + (spd ? 'collecting' : 'idle');
    runStatus.textContent = spd ? 'Sweeping' : 'Idle';
  }

  function renderVib(vib, valid) {
    var noSignal = valid === false;
    var active = !noSignal && (vib === true || vib === 1 || vib === '1');
    vibRead.textContent = noSignal ? 'NO SIGNAL' : (active ? 'YES' : 'no');
    vibRead.style.color = noSignal
      ? 'var(--gray-500)'
      : (active ? 'var(--dot-red)' : 'var(--gray-400)');
    var i, b;
    for (i = 0; i < vibBars.length; i++) {
      b = vibBars[i];
      var h;
      if (active) {
        h = 12 + Math.random() * 10;
        b.style.setProperty('--h', h.toFixed(1) + 'px');
        b.classList.add('on');
      } else {
        h = 2 + Math.random() * 5;
        b.style.setProperty('--h', h.toFixed(1) + 'px');
        b.classList.remove('on');
      }
    }
  }

  function renderSysStatus(d) {
    var temp = d.temp, volt = d.voltage;
    var vibOn = (d.vibration === true || d.vibration === 1 || d.vibration === '1');
    var vibValid = d.vibration_valid !== false;
    var inaOk = d.ina_ok !== false;
    var tempOk = d.temp_valid !== false;
    var alert = (tempOk && typeof temp === 'number' && temp > 50) ||
                (inaOk && typeof volt === 'number' && volt > 0 && volt < 11.5) ||
                (vibValid && vibOn) ||
                !inaOk || !tempOk;
    sysStatus.className = 'test-sys-ok' + (alert ? ' test-sys-alert' : '');
    sysStatus.textContent = (!inaOk && !tempOk)
      ? 'ALARM \u2014 INA219 & temp probe not found'
      : (!inaOk
        ? 'ALARM \u2014 INA219 not found'
        : (!tempOk
          ? 'ALARM \u2014 temp probe not found'
          : (alert
            ? 'ALARM \u2014 check sensor reading'
            : 'All systems nominal')));
  }

  // ── Data freshness (real vs stale vs nothing) ────────
  function sensorValueEls() {
    return document.querySelectorAll('#telemetry-panel .cal-sensor-value');
  }

  function blankTelemetry() {
    sensorValueEls().forEach(function (el) {
      el.textContent = el.id === 't-speed'
        ? String(motorSpeeds[selectedMotor - 1])
        : '--';
    });
    document.getElementById('t-health-bar').style.width = '0%';
    document.getElementById('t-health-bar').classList.remove('low');
    sysStatus.className = 'test-sys-ok';
    sysStatus.textContent = 'No live data \u2014 awaiting ' + MOTOR_NAMES[selectedMotor - 1] + ' stream\u2026';
  }

  function updateFreshness() {
    var rxAt = lastRxAt[selectedMotor];
    var age = rxAt ? (performance.now() - rxAt) / 1000 : null;
    var label = MOTOR_NAMES[selectedMotor - 1];
    var state = 'waiting';
    if (age !== null) {
      state = age <= FRESH_LIVE_MS ? 'live'
            : (age <= FRESH_TIMEOUT_MS ? 'stale' : 'waiting');
    }

    var live = state === 'live';
    var stale = state === 'stale';

    teleDot.className = 'status-dot ' +
      (live ? 'dot-green pulse' : (stale ? 'dot-yellow' : 'dot-off'));

    if (live) {
      tAgeEl.textContent = 'Live \u00b7 ' + age.toFixed(1) + 's ago';
    } else if (stale) {
      tAgeEl.textContent = 'Stale \u00b7 ' + Math.round(age) + 's ago';
    } else {
      tAgeEl.textContent = 'Waiting for ' + label + ' stream\u2026';
    }

    sensorValueEls().forEach(function (el) {
      el.classList.toggle('stale', stale);
    });

    if (state === 'waiting') {
      blankTelemetry();
      renderVib(false);
    } else if (live && lastSample[selectedMotor]) {
      renderSysStatus(lastSample[selectedMotor]);
    } else if (stale) {
      sysStatus.className = 'test-sys-ok test-sys-alert';
      sysStatus.textContent = 'Telemetry stale \u2014 last sample ' +
        Math.round(age) + 's ago';
      renderVib(false);
    }
  }

  // ── Motor control ─────────────────────────────────────
  function sendMotorCommand(speed) {
    var xhr = new XMLHttpRequest();
    xhr.open('POST', API_BASE + '/api/motor/control', true);
    xhr.setRequestHeader('Content-Type', 'application/json');
    xhr.send(JSON.stringify({ motor: selectedMotor, speed: speed }));
  }

  function scheduleSend(value) {
    if (sendTimer) clearTimeout(sendTimer);
    sendTimer = setTimeout(function () {
      sendTimer = null;
      sendMotorCommand(value);
    }, 80);
  }

  function updateMotorUI(value) {
    value = Math.max(-255, Math.min(255, value));
    var mag = Math.abs(value) / 255;
    var glow = (mag * 20).toFixed(1);
    row.style.setProperty('--mag', mag.toFixed(3));
    row.style.setProperty('--glow', glow + 'px');

    var fill = row.querySelector('.motor-control-fill');
    fill.classList.toggle('rev', value < 0);
    fill.classList.toggle('fwd', value > 0);

    row.classList.toggle('on', value !== 0);
    valEl.classList.toggle('on', value !== 0);
    valEl.textContent = value;

    var revLabel = row.querySelector('[data-side="rev"]');
    var fwdLabel = row.querySelector('[data-side="fwd"]');
    revLabel.classList.toggle('on', value < 0);
    fwdLabel.classList.toggle('on', value > 0);

    motorSpeeds[selectedMotor - 1] = value;
  }

  function easeOutExpo(t) {
    return t >= 1 ? 1 : 1 - Math.pow(2, -10 * t);
  }

  function animateMotorTo(target, duration) {
    var start = parseInt(slider.value, 10);
    if (start === target) { updateMotorUI(target); return; }
    if (motorAnim) cancelAnimationFrame(motorAnim);
    var t0 = null;
    function frame(ts) {
      if (t0 === null) t0 = ts;
      var p = Math.min(1, (ts - t0) / duration);
      var val = Math.round(start + (target - start) * easeOutExpo(p));
      slider.value = val;
      updateMotorUI(val);
      if (p < 1) {
        motorAnim = requestAnimationFrame(frame);
      } else {
        motorAnim = null;
        sendMotorCommand(target);
      }
    }
    motorAnim = requestAnimationFrame(frame);
  }

  function setControlsEnabled(enabled) {
    slider.disabled = !enabled;
    offBtn.disabled = !enabled;
    presetBtns.forEach(function (b) { b.disabled = !enabled; });
    motorTabs.forEach(function (t) { t.disabled = !enabled; });
    row.classList.toggle('disabled', !enabled);
  }

  // ── Motor tab switching (same pattern as the detailed page) ──
  function selectMotor(id) {
    if (id === selectedMotor) return;
    if (rampRunning) cancelRamp();
    selectedMotor = id;
    motorTabs.forEach(function (t) {
      t.classList.toggle('active', parseInt(t.getAttribute('data-motor'), 10) === id);
    });
    if (motorTitle) motorTitle.innerHTML = 'Motor ' + MOTOR_NAMES[id - 1] + ' &middot; #' + id;
    if (motorName) motorName.textContent = MOTOR_FULL[id - 1];
    if (row) row.setAttribute('data-motor-row', id);
    if (slider) slider.setAttribute('data-motor', id);
    if (offBtn) offBtn.setAttribute('data-off', id);
    slider.value = motorSpeeds[id - 1];
    updateMotorUI(motorSpeeds[id - 1]);
    blankTelemetry();
    if (lastSample[id]) renderTelemetry(lastSample[id]);
    updateFreshness();
    fetchBaseline();
  }

  // ── Ramp test ─────────────────────────────────────────
  function rampLabel(speed) {
    var pct = Math.round(Math.abs(speed) / 255 * 100);
    return speed > 0 ? 'FWD ' + pct + '%' : (speed < 0 ? 'REV ' + pct + '%' : 'STOP');
  }

  function rampTick() {
    var elapsed = performance.now() - rampStepStart;
    if (elapsed >= RAMP_STEP_MS) {
      rampIndex++;
      if (rampIndex >= RAMP_SEQ.length) {
        cancelRamp(true);
        return;
      }
      rampStepStart = performance.now();
      animateMotorTo(RAMP_SEQ[rampIndex], RAMP_GLIDE_MS);
    }
    var rem = Math.max(0, Math.ceil((RAMP_STEP_MS - (performance.now() - rampStepStart)) / 1000));
    rampStatus.textContent =
      'Ramping \u2192 ' + rampLabel(RAMP_SEQ[rampIndex]) + ' \u00b7 ' + rem + 's';
  }

  function startRamp() {
    if (calibrating || rampRunning) return;
    rampRunning = true;
    rampIndex = 0;
    rampStepStart = performance.now();
    rampStatus.classList.add('running');
    animateMotorTo(RAMP_SEQ[0], RAMP_GLIDE_MS);
    rampTimer = setInterval(rampTick, 250);
    setControlsEnabled(false);
    runStatus.className = 'cal-motor-status collecting';
    runStatus.textContent = 'Ramp';
  }

  function cancelRamp(finished) {
    if (!rampRunning) return;
    rampRunning = false;
    if (rampTimer) { clearInterval(rampTimer); rampTimer = null; }
    rampStatus.classList.remove('running');
    rampStatus.textContent = 'Ramp cancelled';
    if (finished) {
      rampStatus.textContent = 'Ramp complete \u2014 motor stopped';
    }
    setControlsEnabled(!calibrating);
    renderRealStatus();
  }

  // ── Calibration guard ─────────────────────────────────
  function updateCalGuard(status) {
    var was = calibrating;
    calibrating = status.state && status.state !== 'idle';
    if (calibrating) cancelRamp();

    guard.classList.toggle('show', calibrating);
    chipCal.style.background = calibrating ? 'var(--dot-yellow)' : 'var(--gray-500)';
    chipCalLabel.textContent = calibrating
      ? 'Calibration \u2014 ' + status.state
      : 'Calibration idle';

    if (calibrating !== was || !calibrating) {
      setControlsEnabled(!calibrating);
      if (!calibrating) {
        renderRealStatus();
        fetchBaseline();
      }
    }
  }

  function fetchBaseline() {
    var xhr = new XMLHttpRequest();
    xhr.open('GET', API_BASE + '/api/calibration', true);
    xhr.onload = function () {
      if (xhr.status !== 200) return;
      try {
        var bl = (JSON.parse(xhr.responseText).baselines) || {};
        hasBaseline = !!(bl[String(selectedMotor)] || bl[selectedMotor]);
      } catch (e) { hasBaseline = false; }
    };
    xhr.send();
  }

  function pollCalibration() {
    var xhr = new XMLHttpRequest();
    xhr.open('GET', API_BASE + '/api/calibration/status', true);
    xhr.onload = function () {
      if (xhr.status === 200) {
        try { updateCalGuard(JSON.parse(xhr.responseText)); } catch (e) { /* ignore */ }
      }
    };
    xhr.send();
  }

  function pollDeviceStatus() {
    var xhr = new XMLHttpRequest();
    xhr.open('GET', API_BASE + '/api/device/status', true);
    xhr.onload = function () {
      if (xhr.status === 200) {
        try { applyDeviceStatus(JSON.parse(xhr.responseText).online); } catch (e) { /* ignore */ }
      }
    };
    xhr.onerror = function () { applyDeviceStatus(false); };
    xhr.send();
  }

  // ── Event listeners ───────────────────────────────────
  motorTabs.forEach(function (tab) {
    tab.addEventListener('click', function () {
      selectMotor(parseInt(tab.getAttribute('data-motor'), 10));
    });
  });

  slider.addEventListener('input', function () {
    if (rampRunning) cancelRamp();
    updateMotorUI(parseInt(slider.value, 10));
    scheduleSend(parseInt(slider.value, 10));
  });
  slider.addEventListener('change', function () {
    sendMotorCommand(parseInt(slider.value, 10));
  });

  offBtn.addEventListener('click', function () {
    if (rampRunning) cancelRamp();
    animateMotorTo(0, 280);
  });

  presetBtns.forEach(function (btn) {
    btn.addEventListener('click', function () {
      if (rampRunning) cancelRamp();
      animateMotorTo(parseInt(btn.getAttribute('data-speed'), 10), 300);
    });
  });

  rampBtn.addEventListener('click', function () {
    if (rampRunning) { cancelRamp(); return; }
    startRamp();
  });

  // ── Init ──────────────────────────────────────────────
  // Deep-link: ?motor=1-4 preselects the tab (search palette, bookmarks).
  // Invalid or missing values keep the RR default; nothing else changes.
  try {
    var deepMotor = parseInt(new URLSearchParams(window.location.search).get('motor'), 10);
    if (deepMotor >= 1 && deepMotor <= 4) selectMotor(deepMotor);
  } catch (e) { /* ignore, defaults stand */ }
  buildBeats();
  updateMotorUI(0);
  setControlsEnabled(true);
  pollCalibration();
  pollDeviceStatus();
  fetchBaseline();
  connectWS();
  updateFreshness();
  pollTimer = setInterval(pollCalibration, 2000);
  freshTimer = setInterval(updateFreshness, 1000);
  setInterval(pollDeviceStatus, 3000);
})();