(function () {
  'use strict';

  // ── Config ────────────────────────────────────────────
  // Auto-detect backend server from page location (fixes multi-PC access)
  var WS_URL = (window.location.protocol === 'https:' ? 'wss:' : 'ws:') + 
               '//' + window.location.hostname + ':8000/ws';
  var API_BASE = (window.location.protocol === 'https:' ? 'https:' : 'http:') + 
                 '//' + window.location.hostname + ':8000';
  var MOTOR_NAMES = ['FL', 'FR', 'RL', 'RR'];

  // ── State ─────────────────────────────────────────────
  var ws = null;
  var calState = 'idle';          // idle | warmup | collecting
  var warmupTimer = null;
  var uiTimer = null;
  var samplesCollected = 0;
  var latestMotorData = {};       // { motorId: {temp, voltage, current, vibration, health} }
  var motorLastRx = {};           // motorId -> performance.now() of last fresh packet
  var FRESH_LIVE_MS = 5000;
  var isPaused = false;
  var deviceOnline = false;       // ESP32 presence (real heartbeat, not server WS)
  var statusCls = 'status-dot dot-green';
  var statusText = 'Ready';

  // ── DOM refs ──────────────────────────────────────────
  var startBtn = document.getElementById('cal-start-btn');
  var downloadBtn = document.getElementById('cal-download-btn');
  var resetBtn = document.getElementById('cal-reset-btn');
  var statusDot = document.getElementById('cal-status-dot');
  var statusLabel = document.getElementById('cal-status-label');
  var warmupRow = document.getElementById('warmup-row');
  var warmupFill = document.getElementById('warmup-progress-fill');
  var warmupText = document.getElementById('warmup-progress-text');
  var collectRow = document.getElementById('collect-row');
  var collectFill = document.getElementById('collect-progress-fill');
  var collectText = document.getElementById('collect-progress-text');
  var phaseLabel = document.getElementById('cal-phase-label');
  var samplesLabel = document.getElementById('cal-samples-label');
  var connLabel = document.getElementById('cal-conn-label');
  var connBanner = document.getElementById('conn-lost-banner');
  var motorGrid = document.getElementById('cal-motor-grid');
  var motorControlGrid = document.getElementById('motor-control-grid');
  var motorControlHint = document.getElementById('motor-control-hint');
  var modeToggleBtn = document.getElementById('mode-toggle-btn');
  var modeToggleLabel = document.getElementById('mode-toggle-label');
  var sweepStatusRow = document.getElementById('sweep-status-row');
  var sweepStatusLabel = document.getElementById('sweep-status-label');
  var sweepStatusFill = document.getElementById('sweep-status-fill');
  var sweepStatusText = document.getElementById('sweep-status-text');
  var histPrev = document.getElementById('hist-prev');
  var histNext = document.getElementById('hist-next');
  var schedDays = document.getElementById('sched-days');
  var schedList = document.getElementById('sched-list');
  var histFootTs = document.getElementById('hist-foot-ts');
  var histToday = document.getElementById('hist-today');
  var histMore = document.getElementById('hist-more');
  var motorSpeeds = [0, 0, 0, 0];   // last sent speed per motor

  // Control mode: 'auto' (sweep drives motors, sliders hidden) or
  // 'manual' (sliders visible, user drives motors directly)
  var mode = 'auto';

  // History timeline state
  var histDay = new Date();
  var histTimer = null;
  var expandedHours = {};   // dayKey:hourKey -> expanded (survives re-renders)

  // ── WebSocket ─────────────────────────────────────────
  function connectWS() {
    if (ws && ws.readyState === WebSocket.OPEN) return;
    ws = new WebSocket(WS_URL);

    ws.onopen = function () {
      setConnectionStatus(true);
      ws.send('ping');
    };

    ws.onmessage = function (e) {
      try {
        var msg = JSON.parse(e.data);
        handleWSMessage(msg);
      } catch (err) {
        // ignore
      }
    };

    ws.onclose = function () {
      setConnectionStatus(false);
      if (calState !== 'idle') {
        showLossBanner(true);
      }
      ws = null;
      setTimeout(connectWS, 3000);
    };

    ws.onerror = function () {
      ws.close();
    };
  }

  function handleWSMessage(msg) {
    if (msg.type === 'motor_update') {
      var mId = msg.motor;
      motorLastRx[mId] = performance.now();
      latestMotorData[mId] = {
        temp: msg.temp,
        voltage: msg.voltage,
        current: msg.current,
        vibration: msg.vibration,
        health: msg.health,
        vibration_valid: msg.vibration_valid !== false,
        ina_ok: msg.ina_ok !== false,
        temp_valid: msg.temp_valid !== false,
      };
      if (calState === 'sweep' || calState === 'warmup') {
        queueMotorRender();
      }
    } else if (msg.type === 'motor_control') {
      // Motor control command from any client (broadcast from backend)
      // Update motor speed so all clients see which motors are running
      if (msg.motor > 0 && msg.motor <= 4) {
        motorSpeeds[msg.motor - 1] = msg.speed;
      }
    } else if (msg.type === 'user_joined' || msg.type === 'user_left' || msg.type === 'user_presence') {
      // Pass user presence messages through to global script.js
      // (handled by window.dashboardWS which is the same connection)
    } else if (msg.type === 'snapshot') {
      // Cached history replay — deliberately ignored for display. Only live
      // motor_update packets (real hardware samples) drive the cards, so an
      // unplugged/disconnected sensor never leaves stale or fake values.
    } else if (msg.type === 'calibration_status') {
      updateCalState(msg);
    } else if (msg.type === 'device_status') {
      applyDeviceStatus(msg.online);
    } else if (msg.type === 'pong') {
      // keep alive
    }
  }

  function setStatusBadge(cls, text) {
    statusCls = cls;
    statusText = text;
    renderStatusBadge();
  }

  function renderStatusBadge() {
    if (!statusDot || !statusLabel) return;
    if (!deviceOnline) {
      statusDot.className = 'status-dot dot-red';
      statusLabel.textContent = 'Offline';
      return;
    }
    statusDot.className = statusCls;
    statusLabel.textContent = statusText;
  }

  function applyDeviceStatus(online) {
    deviceOnline = online === true;
    renderStatusBadge();
  }

  function setConnectionStatus(connected) {
    if (connected) {
      connLabel.textContent = 'Connected to server';
      connBanner.style.display = 'none';
    } else {
      connLabel.textContent = 'Disconnected from server';
      statusDot.className = 'status-dot dot-red';
      statusLabel.textContent = 'Disconnected';
    }
  }

  function showLossBanner(show) {
    connBanner.style.display = show ? 'block' : 'none';
  }

  // ── Calibration state updates from backend ────────────
  function updateCalState(status) {
    var prevState = calState;
    calState = status.state || 'idle';
    samplesCollected = status.samples_collected || 0;

    if (calState === 'warmup') {
      var remaining = Math.max(0, Math.round(status.warmup_remaining_s || 0));
      var mins = Math.floor(remaining / 60);
      var secs = remaining % 60;
      var pct = status.warmup_pct || 0;

      warmupRow.style.display = 'flex';
      warmupFill.style.width = pct + '%';
      warmupText.textContent = mins + 'm ' + secs + 's remaining';
      collectRow.style.display = 'none';
      sweepStatusRow.style.display = 'none';

      phaseLabel.textContent = 'Warming up — no data recorded';
      setStartPauseBtn(true);
      setStatusBadge('status-dot dot-yellow pulse', 'Warming up ' + pct + '%');
    } else if (calState === 'sweep') {
      warmupRow.style.display = 'none';
      collectRow.style.display = 'none';
      sweepStatusRow.style.display = 'flex';

      var spd = status.speed_pct || 0;
      var dir = status.direction > 0 ? 'FWD' : (status.direction < 0 ? 'REV' : '--');
      var remS = status.step_remaining_s || 0;
      var cyc = status.cycle || 1;

      sweepStatusLabel.textContent = 'Sweep ' + spd + '%';
      sweepStatusFill.style.width = Math.min(100, spd) + '%';
      sweepStatusText.textContent =
        dir + ' \u00b7 ' + remS + 's left \u00b7 cycle ' + cyc;

      phaseLabel.textContent = 'Sweeping speeds — recording data';
      setStartPauseBtn(true);
      setStatusBadge('status-dot dot-green pulse', 'Sweeping ' + spd + '%');
    } else {
      // idle
      warmupRow.style.display = 'none';
      collectRow.style.display = 'none';
      sweepStatusRow.style.display = 'none';
      phaseLabel.textContent = 'Press Start to begin';
      setStartPauseBtn(false);
      setStatusBadge('status-dot dot-green', 'Ready');
    }

    samplesLabel.textContent = samplesCollected + ' samples recorded';
    applyModeVisibility();
    refreshHistoryLoop();

    if (prevState !== calState) {
      renderMotorCards();
    }
  }

  // Sliders only show in MANUAL mode while idle; calibration forces AUTO.
  function applyModeVisibility() {
    var calibrating = calState !== 'idle';
    var showSliders = !calibrating && mode === 'manual';

    motorControlGrid.style.display = showSliders ? 'grid' : 'none';
    modeToggleLabel.textContent = calibrating ? 'AUTO' : mode.toUpperCase();
    modeToggleBtn.classList.toggle('mode-on', showSliders);
    modeToggleBtn.disabled = calibrating;
    motorControlHint.textContent = calibrating
      ? 'Motors auto-run during calibration'
      : (showSliders
          ? 'Sliders send speed to the ESP32 (L298N)'
          : 'AUTO mode — press Start to run the sweep');
    setMotorControlsEnabled(showSliders);
  }

  function setStartPauseBtn(isActive) {
    var content = startBtn.querySelector('.btn-content');
    if (isActive) {
      content.innerHTML =
        '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/></svg> Pause';
    } else {
      content.innerHTML =
        '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg> Start';
    }
  }

  // ── Motor cards ───────────────────────────────────────
  // Coalesce card renders: motor messages arrive ~4x/sec (250ms sampling);
  // rebuild the DOM at most every 500ms so the page stays clickable.
  var motorRenderTimer = null;
  function queueMotorRender() {
    if (motorRenderTimer) return;
    motorRenderTimer = setTimeout(function () {
      motorRenderTimer = null;
      renderMotorCards();
    }, 500);
  }

  function renderMotorCards() {
    if (calState === 'idle' && Object.keys(latestMotorData).length === 0) {
      var emptyHtml = '';
      MOTOR_NAMES.forEach(function (name) {
        emptyHtml +=
          '<div class="cal-motor-card">' +
          '<div class="cal-motor-header">' +
          '<span class="cal-motor-name">Motor ' + name + '</span>' +
          '<span class="cal-motor-status idle">Idle</span>' +
          '</div>' +
          '<div class="cal-sensor-row"><span class="cal-sensor-label">Temperature</span><span class="cal-sensor-value">-- \u00b0C</span></div>' +
          '<div class="cal-sensor-row"><span class="cal-sensor-label">Voltage</span><span class="cal-sensor-value">-- V</span></div>' +
          '<div class="cal-sensor-row"><span class="cal-sensor-label">Current</span><span class="cal-sensor-value">-- A</span></div>' +
          '<div class="cal-sensor-row"><span class="cal-sensor-label">Vibration</span><span class="cal-sensor-value">--</span></div>' +
          '</div>';
      });
      motorGrid.innerHTML = emptyHtml;
      return;
    }

    var html = '';
    MOTOR_NAMES.forEach(function (name, i) {
      var mId = i + 1;
      var d = latestMotorData[mId];
      var fresh = motorLastRx[mId] !== undefined &&
        (performance.now() - motorLastRx[mId]) < FRESH_LIVE_MS;
      // Stale or never-heard-from motors must render as no-data, not as
      // cached/stale values.
      var live = fresh && d;
      var statusClass = calState === 'idle' ? 'idle' : (calState === 'sweep' ? 'collecting' : 'warmup');
      var statusText = calState === 'idle' ? 'Idle' : (calState === 'sweep' ? 'Sweeping' : 'Warming');

      var tempStr = (live && d.temp_valid && typeof d.temp === 'number')
        ? d.temp.toFixed(1) + ' \u00b0C' : '-- \u00b0C';
      var voltStr = (live && d.ina_ok && typeof d.voltage === 'number')
        ? d.voltage.toFixed(2) + ' V' : '-- V';
      var currStr = (live && d.ina_ok && typeof d.current === 'number')
        ? d.current.toFixed(3) + ' A' : '-- A';
      var vibStr = '--';
      if (live) {
        if (d.vibration_valid) {
          vibStr = d.vibration ? 'YES' : 'no';
        } else {
          vibStr = 'NO SIGNAL';
        }
      }
      var healthStr = (live && typeof d.health === 'number')
        ? d.health.toFixed(1) + '%' : '--';

      html += '<div class="cal-motor-card">';
      html += '<div class="cal-motor-header">';
      html += '<span class="cal-motor-name">Motor ' + name + '</span>';
      html += '<span class="cal-motor-status ' + statusClass + '">' + statusText + '</span>';
      html += '</div>';
      html +=
        '<div class="cal-sensor-row"><span class="cal-sensor-label">Temperature</span><span class="cal-sensor-value">' +
        tempStr + '</span></div>';
      html +=
        '<div class="cal-sensor-row"><span class="cal-sensor-label">Voltage</span><span class="cal-sensor-value">' +
        voltStr + '</span></div>';
      html +=
        '<div class="cal-sensor-row"><span class="cal-sensor-label">Current</span><span class="cal-sensor-value">' +
        currStr + '</span></div>';
      html +=
        '<div class="cal-sensor-row"><span class="cal-sensor-label">Vibration</span><span class="cal-sensor-value">' +
        vibStr + '</span></div>';
      html +=
        '<div class="cal-sensor-row"><span class="cal-sensor-label">Health</span><span class="cal-sensor-value">' +
        healthStr + '</span></div>';
      if (live && typeof d.health === 'number') {
        var hpct = Math.min(100, Math.max(0, d.health));
        html +=
          '<div style="margin-top:8px;height:3px;border-radius:9999px;background:var(--bg-white-05);overflow:hidden;">' +
          '<div style="height:100%;width:' + hpct +
          '%;border-radius:9999px;background:var(--primary);transition:width 0.5s ease;"></div></div>';
      }
      html += '</div>';
    });
    motorGrid.innerHTML = html;
  }

  // ── Manual motor control ──────────────────────────────
  var motorAnim = {};         // per-motor rAF handles
  var motorSendTimers = {};   // per-motor debounce timers

  function sendMotorCommand(motor, speed) {
    var xhr = new XMLHttpRequest();
    xhr.open('POST', API_BASE + '/api/motor/control', true);
    xhr.setRequestHeader('Content-Type', 'application/json');
    xhr.send(JSON.stringify({ motor: motor, speed: speed }));
  }

  // Throttle rapid drag updates, then send the latest value
  function scheduleMotorSend(motor, value) {
    if (motorSendTimers[motor]) clearTimeout(motorSendTimers[motor]);
    motorSendTimers[motor] = setTimeout(function () {
      motorSendTimers[motor] = null;
      sendMotorCommand(motor, value);
    }, 80);
  }

  // Reflect a slider value on the row: fill, glow, direction, labels, readout
  function updateMotorUI(motor, value) {
    value = Math.max(-255, Math.min(255, value));
    var row = motorControlGrid.querySelector('[data-motor-row="' + motor + '"]');
    if (!row) return;

    var mag = Math.abs(value) / 255;
    var glow = (mag * 20).toFixed(1);

    row.style.setProperty('--mag', mag.toFixed(3));
    row.style.setProperty('--glow', glow + 'px');

    var fill = row.querySelector('.motor-control-fill');
    if (fill) {
      fill.classList.toggle('rev', value < 0);
      fill.classList.toggle('fwd', value > 0);
    }

    row.classList.toggle('on', value !== 0);

    var valEl = document.getElementById('motor-val-' + motor);
    if (valEl) {
      valEl.classList.toggle('on', value !== 0);
      valEl.textContent = value;
    }

    var revLabel = row.querySelector('[data-side="rev"]');
    var fwdLabel = row.querySelector('[data-side="fwd"]');
    if (revLabel) revLabel.classList.toggle('on', value < 0);
    if (fwdLabel) fwdLabel.classList.toggle('on', value > 0);

    motorSpeeds[motor - 1] = value;
  }

  function easeOutExpo(t) {
    return t >= 1 ? 1 : 1 - Math.pow(2, -10 * t);
  }

  // Glide a motor's slider from its current value to `target` and send once at the end
  function animateMotorTo(motor, target, duration) {
    var slider = document.getElementById('motor-slider-' + motor);
    if (!slider) return;

    var start = parseInt(slider.value, 10);
    if (start === target) {
      updateMotorUI(motor, target);
      return;
    }

    if (motorAnim[motor]) cancelAnimationFrame(motorAnim[motor]);

    var t0 = null;
    function frame(ts) {
      if (t0 === null) t0 = ts;
      var p = Math.min(1, (ts - t0) / duration);
      var val = Math.round(start + (target - start) * easeOutExpo(p));
      slider.value = val;
      updateMotorUI(motor, val);
      if (p < 1) {
        motorAnim[motor] = requestAnimationFrame(frame);
      } else {
        motorAnim[motor] = null;
        sendMotorCommand(motor, target);
      }
    }
    motorAnim[motor] = requestAnimationFrame(frame);
  }

  function setMotorControlsEnabled(enabled) {
    var sliders = motorControlGrid.querySelectorAll('input[type="range"]');
    sliders.forEach(function (s) { s.disabled = !enabled; });
    var offBtns = motorControlGrid.querySelectorAll('.btn-off');
    offBtns.forEach(function (b) { b.disabled = !enabled; });
    motorControlGrid.querySelectorAll('.motor-control-row').forEach(function (r) {
      r.classList.toggle('disabled', !enabled);
    });
  }

  function renderMotorControls() {
    var html = '';
    MOTOR_NAMES.forEach(function (name, i) {
      var mId = i + 1;
      html += '<div class="motor-control-row" data-motor-row="' + mId + '">';
      html += '<div class="motor-control-head">';
      html += '<span class="cal-motor-name">Motor ' + name + '</span>';
      html += '<div class="motor-control-meta">';
      html += '<span class="motor-control-val" id="motor-val-' + mId + '">0</span>';
      html += '<button class="btn-off" type="button" data-off="' + mId + '" title="Stop motor"><span class="btn-off-label">OFF</span></button>';
      html += '</div>';
      html += '</div>';
      html += '<div class="motor-control-track">';
      html += '<div class="motor-control-groove"></div>';
      html += '<div class="motor-control-fill"></div>';
      html += '<div class="motor-control-tick"></div>';
      html += '<input type="range" class="motor-control-input" min="-255" max="255" step="1" value="0" ';
      html += 'id="motor-slider-' + mId + '" data-motor="' + mId + '">';
      html += '</div>';
      html += '<div class="motor-control-labels">';
      html += '<span class="motor-label" data-side="rev">REV</span>';
      html += '<span class="motor-label" data-side="fwd">FWD</span>';
      html += '</div>';
      html += '</div>';
    });
    motorControlGrid.innerHTML = html;

    motorControlGrid.querySelectorAll('input[type="range"]').forEach(function (slider) {
      var motor = parseInt(slider.getAttribute('data-motor'), 10);
      slider.addEventListener('input', function () {
        if (motorAnim[motor]) {
          cancelAnimationFrame(motorAnim[motor]);
          motorAnim[motor] = null;
        }
        var val = parseInt(slider.value, 10);
        updateMotorUI(motor, val);
        scheduleMotorSend(motor, val);
      });
      slider.addEventListener('change', function () {
        sendMotorCommand(motor, parseInt(slider.value, 10));
      });
    });

    motorControlGrid.querySelectorAll('.btn-off').forEach(function (btn) {
      btn.addEventListener('click', function () {
        var motor = parseInt(btn.getAttribute('data-off'), 10);
        animateMotorTo(motor, 0, 280);
      });
    });
  }

  // ── UI Refresh (poll calibration status) ─────────────
  function pollStatus() {
    if (calState === 'idle') return;
    var xhr = new XMLHttpRequest();
    xhr.open('GET', API_BASE + '/api/calibration/status', true);
    xhr.onload = function () {
      if (xhr.status === 200) {
        var status = JSON.parse(xhr.responseText);
        updateCalState(status);
      }
    };
    xhr.send();
  }

  // ── Device presence fallback poll (also updated via WS device_status) ──
  function pollDeviceStatus() {
    var xhr = new XMLHttpRequest();
    xhr.open('GET', API_BASE + '/api/device/status', true);
    xhr.onload = function () {
      if (xhr.status === 200) {
        try { applyDeviceStatus(JSON.parse(xhr.responseText).online); } catch (e) { /* ignore */ }
      }
    };
    xhr.onerror = function () {
      applyDeviceStatus(false);
    };
    xhr.send();
  }

  // ── Actions ───────────────────────────────────────────
  function toggleCalibration() {
    if (calState !== 'idle') {
      // Pause / Stop
      var xhr = new XMLHttpRequest();
      xhr.open('POST', API_BASE + '/api/calibration/stop', true);
      xhr.onload = function () {
        if (xhr.status === 200) {
          var result = JSON.parse(xhr.responseText);
          samplesCollected = result.samples_collected || 0;
          calState = 'idle';
          updateCalState({ state: 'idle' });
          showLossBanner(false);
        }
      };
      xhr.send();
    } else {
      // Start
      var xhr = new XMLHttpRequest();
      xhr.open('POST', API_BASE + '/api/calibration/start', true);
      xhr.onload = function () {
        if (xhr.status === 200) {
          pollStatus();
        }
      };
      xhr.send();
    }
  }

  function downloadData() {
    window.open(API_BASE + '/api/calibration/export', '_blank');
  }

  function resetCalibration() {
    if (calState !== 'idle') {
      // Stop first, then clear
      var xhr = new XMLHttpRequest();
      xhr.open('POST', API_BASE + '/api/calibration/stop', true);
      xhr.onload = function () {
        doReset();
      };
      xhr.onerror = function () {
        doReset();
      };
      xhr.send();
    } else {
      doReset();
    }
  }

  function doReset() {
    warmupRow.style.display = 'none';
    collectRow.style.display = 'none';
    phaseLabel.textContent = 'Press Start to begin';
    setStartPauseBtn(false);
    setStatusBadge('status-dot dot-green', 'Ready');
    samplesLabel.textContent = '0 samples collected';
    calState = 'idle';
    samplesCollected = 0;
    latestMotorData = {};
    showLossBanner(false);
    renderMotorCards();
  }

  // ── History timeline (reanime-style, past data) ──────
  function pad2(n) { return (n < 10 ? '0' : '') + n; }

  function dateStr(d) {
    return d.getFullYear() + '-' + pad2(d.getMonth() + 1) + '-' + pad2(d.getDate());
  }

  function shiftDay(delta) {
    var d = new Date(histDay);
    d.setDate(d.getDate() + delta);
    return d;
  }

  var WEEKDAYS = ['SUN', 'MON', 'TUE', 'WED', 'THU', 'FRI', 'SAT'];

  function fetchJSON(url, cb) {
    var xhr = new XMLHttpRequest();
    xhr.open('GET', API_BASE + url, true);
    xhr.onload = function () {
      if (xhr.status === 200) {
        try { cb(JSON.parse(xhr.responseText)); } catch (e) { cb(null); }
      } else {
        cb(null);
      }
    };
    xhr.onerror = function () { cb(null); };
    xhr.send();
  }

  function renderDayNav(days) {
    var html = '';
    for (var off = -1; off <= 1; off++) {
      var d = shiftDay(off);
      var key = dateStr(d);
      var isSel = key === dateStr(histDay);
      var hasData = days && days.indexOf(key) >= 0;
      html +=
        '<button class="day-tile' + (isSel ? ' active' : '') +
        (hasData ? ' has-data' : '') + '" data-day="' + key + '" type="button">' +
        '<span class="day-label">' + WEEKDAYS[d.getDay()] + '</span>' +
        '<span class="h-badge"><span class="h-badge-inner">' + d.getDate() + '</span></span>' +
        '</button>';
    }
    schedDays.innerHTML = html;

    schedDays.querySelectorAll('.day-tile').forEach(function (tile) {
      tile.addEventListener('click', function () {
        histDay = new Date(tile.getAttribute('data-day'));
        reloadHistory();
      });
    });
  }

  var HEAT_OK = 45, HEAT_WARN = 50;

  function heatCls(temp) {
    if (temp >= HEAT_WARN) return 'alarm';
    if (temp >= HEAT_OK) return 'warn';
    return 'ok';
  }

  function renderHourlySummary(segments) {
    var hours = aggregateHours(segments);
    renderActivityStrip(hours);

    if (!hours || hours.length === 0) {
      schedList.innerHTML =
        '<div class="sched-empty">No data recorded on this day</div>';
      return;
    }

    var now = Date.now() / 1000;
    var nowHour = Math.floor(now / 3600);
    var isToday = dateStr(histDay) === dateStr(new Date());
    var nowIdx = -1;
    var todayKey = dateStr(histDay);

    var rows = [];
    hours.forEach(function (h) {
      if (isToday && nowIdx < 0 && h.key > nowHour) {
        nowIdx = rows.length;
      }
      var expanded = !!expandedHours[todayKey + ':' + h.key];

      var motors = Object.keys(h.motors).sort(function (a, b) { return a - b; })
        .map(function (m) {
          return MOTOR_NAMES[(m - 1)] || ('M' + m);
        }).join(' \u00b7 ');
      var title = (motors || 'Motors') + ' \u00b7 ' + h.runs +
        (h.runs > 1 ? ' runs' : ' run') + ' \u00b7 ' + h.minutes + ' min';
      var reading = h.maxTemp > 0
        ? h.maxTemp.toFixed(1) + '\u00b0C max'
        : '--\u00b0C';
      reading += ' \u00b7 ' + h.avgCurr.toFixed(2) + 'A';
      var tag = h.alert ? 'ALERT' : 'OK';

      var row = '<div class="sched-row hour' + (h.alert ? ' is-alert' : '') +
        (expanded ? ' expanded' : '') + '" data-hour="' + h.key + '">';
      if (h.alert) {
        row += '<div class="sched-alert-badge">ALERT</div>';
      }
      row += '<div class="sched-hour-head">';
      row += '<div class="sched-row-main">';
      row += '<span class="sched-time">' + hourLabel(h.key) + '</span>';
      row += '<span class="sched-marker"></span>';
      row += '<span class="sched-title">' + title + '</span>';
      row += '</div>';
      row += '<div class="sched-meta">';
      row += '<span class="sched-tag ' + (h.alert ? 'alert' : '') + '">' + tag + '</span>';
      row += '<span class="sched-reading">' + reading + '</span>';
      row += '<svg class="sched-hour-chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6"></path></svg>';
      row += '</div>';
      row += '</div>';

      row += '<div class="sched-hour-detail">';
      row += '<div class="heat-cells">';
      MOTOR_NAMES.forEach(function (name, i) {
        var mid = i + 1;
        var m = h.motors[mid];
        row += '<div class="heat-cell">';
        row += '<span class="heat-label">' + name + '</span>';
        row += '<div class="heat-track"><div class="heat-fill' +
          (m ? ' ' + heatCls(m.maxTemp) : ' none') +
          '" style="width:' + (m ? Math.min(100, m.seconds / 36) : 0) + '%;"></div></div>';
        row += '</div>';
      });
      row += '</div>';

      row += '<div class="metric-bars">';
      row += metricBar('T', (h.avgTemp / 60) * 100, h.avgTemp.toFixed(1) + '\u00b0', 83.3, h.avgTemp > 50);
      row += metricBar('A', (h.avgCurr / 3) * 100, h.avgCurr.toFixed(2) + 'A', 66.7, h.avgCurr > 2);
      row += metricBar('V', ((h.avgVolt - 10) / 4) * 100, h.avgVolt.toFixed(1) + 'V', 37.5, h.avgVolt > 0 && h.avgVolt < 11.5);
      if (h.vib > 0) {
        row += '<span class="metric-vib" title="Vibrations detected">' + h.vib + ' vib</span>';
      }
      row += '</div>';
      row += '</div>';

      row += '<div class="sched-hour-segs">';
      h.segs.forEach(function (s) {
        var segAlert = (s.max_temp > 50) || (s.vibration_count > 0) ||
                       (s.avg_voltage > 0 && s.avg_voltage < 11.5);
        var mName = MOTOR_NAMES[(s.motor - 1)] || ('M' + s.motor);
        var dir = s.direction === 'IDLE' ? 'IDLE' : (s.direction + ' ' + s.speed_pct + '%');
        var dur = (s.duration_s || 0) / 60;
        var segReading = (s.max_temp > 0 ? s.max_temp.toFixed(1) + '\u00b0C max' : '--') +
          ' \u00b7 ' + (s.avg_current || 0).toFixed(2) + 'A';
        if (s.vibration_count > 0) segReading += ' \u00b7 ' + s.vibration_count + ' vib';
        row += '<div class="sched-seg' + (segAlert ? ' is-alert' : '') + '">';
        row += '<span class="sched-seg-motor">' + mName + '</span>';
        row += '<span class="sched-seg-info">' + dir + ' \u00b7 ' + s.start_iso + ' \u00b7 ' +
          dur.toFixed(1) + ' min</span>';
        row += '<span class="sched-seg-reading">' + segReading + '</span>';
        row += '</div>';
      });
      row += '</div>';

      row += '</div>';
      rows.push(row);
    });

    if (isToday && nowIdx < 0) nowIdx = rows.length;
    if (isToday) {
      rows.splice(
        nowIdx, 0,
        '<div class="sched-now"><span class="sched-now-diamond" aria-hidden="true"></span><span class="sched-now-line" aria-hidden="true"></span></div>'
      );
    }

    schedList.innerHTML = rows.join('');
    bindHourExpands();
  }

  function metricBar(label, pct, val, tickPct, danger) {
    var clamped = Math.max(0, Math.min(100, pct));
    var tick = Math.max(0, Math.min(100, tickPct));
    return '<div class="metric-bar' + (danger ? ' danger' : '') + '">' +
      '<span class="metric-label">' + label + '</span>' +
      '<div class="metric-track"><div class="metric-fill" style="width:' + clamped + '%;"></div>' +
      '<span class="metric-tick" style="left:' + tick + '%;"></span></div>' +
      '<span class="metric-val">' + val + '</span></div>';
  }

  function bindHourExpands() {
    var rows = schedList.querySelectorAll('.sched-row.hour');
    rows.forEach(function (r) {
      r.addEventListener('click', function () {
        var key = dateStr(histDay) + ':' + r.getAttribute('data-hour');
        if (r.classList.toggle('expanded')) expandedHours[key] = true;
        else delete expandedHours[key];
      });
    });
  }

  function hourLabel(key) {
    var d = new Date(key * 3600 * 1000);
    return pad2(d.getHours()) + ':00';
  }

  function aggregateHours(segments) {
    var map = {};
    (segments || []).forEach(function (s) {
      var key = Math.floor((s.start_ts || 0) / 3600);
      var h = map[key] || (map[key] = {
        key: key, runs: 0, samples: 0, seconds: 0,
        tempMax: 0, tempSum: 0, voltSum: 0, currSum: 0,
        vib: 0, motors: {}, segs: [], alert: false,
      });
      var n = s.samples || 1;
      h.runs += 1;
      h.samples += n;
      h.seconds += s.duration_s || 0;
      h.tempSum += (s.avg_temp || 0) * n;
      if (s.max_temp > h.tempMax) h.tempMax = s.max_temp;
      h.voltSum += (s.avg_voltage || 0) * n;
      h.currSum += (s.avg_current || 0) * n;
      h.vib += s.vibration_count || 0;

      var m = h.motors[s.motor] || (h.motors[s.motor] = { seconds: 0, maxTemp: 0 });
      m.seconds += s.duration_s || 0;
      if (s.max_temp > m.maxTemp) m.maxTemp = s.max_temp;

      h.segs.push(s);
      if ((s.max_temp > 50) || (s.vibration_count > 0) ||
          (s.avg_voltage > 0 && s.avg_voltage < 11.5)) {
        h.alert = true;
      }
    });

    var hours = Object.keys(map).map(function (k) { return map[k]; });
    hours.sort(function (a, b) { return a.key - b.key; });
    hours.forEach(function (h) {
      h.minutes = Math.round(h.seconds / 60);
      h.avgTemp = h.samples ? h.tempSum / h.samples : 0;
      h.avgVolt = h.samples ? h.voltSum / h.samples : 0;
      h.avgCurr = h.samples ? h.currSum / h.samples : 0;
    });
    return hours;
  }

  function renderActivityStrip(hours) {
    var strip = document.getElementById('sched-strip');
    if (!strip) return;
    var nowHour = Math.floor(Date.now() / 3600);
    var isToday = dateStr(histDay) === dateStr(new Date());
    var byHour = {};
    (hours || []).forEach(function (h) { byHour[h.key] = h; });

    var dayStart = Math.floor(
      new Date(histDay.getFullYear(), histDay.getMonth(), histDay.getDate()).getTime() / 3600000
    );
    var html = '';
    for (var i = 0; i < 24; i++) {
      var key = dayStart + i;
      var h = byHour[key];
      var cls = 'sched-strip-cell';
      var tip = '';
      if (h) {
        cls += h.alert ? ' alert' : ' on';
        tip = ' title="' + hourLabel(key) + ' \u00b7 ' + h.runs +
          (h.runs > 1 ? ' runs' : ' run') + ' \u00b7 ' + h.minutes + ' min"';
      }
      if (isToday && key === nowHour) cls += ' now';
      html += '<div class="' + cls + '"' + tip + '></div>';
    }
    strip.innerHTML = html;
  }

  function renderHistory(daysCache) {
    fetchJSON('/api/history/days', function (days) {
      renderDayNav(days || []);
      fetchJSON('/api/history/segments?day=' + dateStr(histDay), function (res) {
        renderHourlySummary(res ? res.segments : null);
      });
    });
    updateTodayBtn();
    updateFootClock();
  }

  function updateTodayBtn() {
    if (!histToday) return;
    histToday.style.display =
      dateStr(histDay) === dateStr(new Date()) ? 'none' : '';
  }

  function nowLabel() {
    var d = new Date();
    return d.getFullYear() + '/' + pad2(d.getMonth() + 1) + '/' + pad2(d.getDate()) +
      ' ' + pad2(d.getHours()) + ':' + pad2(d.getMinutes()) + ':' + pad2(d.getSeconds());
  }

  function updateFootClock() {
    if (histFootTs) histFootTs.textContent = nowLabel();
  }

  function reloadHistory() {
    renderHistory();
  }

  function refreshHistoryLoop() {
    if (calState !== 'idle') {
      if (!histTimer) {
        renderHistory();
        histTimer = setInterval(renderHistory, 10000);
      }
    } else if (histTimer) {
      clearInterval(histTimer);
      histTimer = null;
    }
  }

  // ── Event listeners ───────────────────────────────────
  startBtn.addEventListener('click', toggleCalibration);
  downloadBtn.addEventListener('click', downloadData);
  resetBtn.addEventListener('click', resetCalibration);

  modeToggleBtn.addEventListener('click', function () {
    if (calState !== 'idle') return;
    mode = (mode === 'manual') ? 'auto' : 'manual';
    applyModeVisibility();
  });

  histPrev.addEventListener('click', function () {
    histDay = shiftDay(-1);
    renderHistory();
  });
  histNext.addEventListener('click', function () {
    histDay = shiftDay(1);
    renderHistory();
  });

  if (histToday) {
    histToday.addEventListener('click', function () {
      histDay = new Date();
      renderHistory();
      if (schedList) schedList.scrollTop = 0;
    });
  }

  if (histMore) {
    histMore.addEventListener('click', function () {
      window.location.href = '../detailed/';
    });
  }

  // Periodic UI refresh
  uiTimer = setInterval(pollStatus, 2000);
  setInterval(pollDeviceStatus, 3000);
  setInterval(updateFootClock, 1000);
  setInterval(function () {
    // Re-render cards when any motor crosses the freshness boundary so
    // stale/offline motors drop to "--" without waiting for a new packet.
    if (calState === 'sweep' || calState === 'warmup') {
      queueMotorRender();
    } else if (Object.keys(latestMotorData).length) {
      renderMotorCards();
    }
  }, 1000);

  // ── Init ──────────────────────────────────────────────
  renderMotorCards();
  renderMotorControls();
  applyModeVisibility();
  renderHistory();
  connectWS();
})();
