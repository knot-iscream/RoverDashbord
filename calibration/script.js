(function () {
  'use strict';

  // ── Config ────────────────────────────────────────────
  var WS_URL = 'ws://localhost:8000/ws';
  var API_BASE = 'http://localhost:8000';
  var MOTOR_NAMES = ['FL', 'FR', 'RL', 'RR'];

  // ── State ─────────────────────────────────────────────
  var ws = null;
  var calState = 'idle';          // idle | warmup | collecting
  var warmupTimer = null;
  var uiTimer = null;
  var samplesCollected = 0;
  var latestMotorData = {};       // { motorId: {temp, voltage, current, vibration, health} }
  var isPaused = false;

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
      latestMotorData[mId] = {
        temp: msg.temp,
        voltage: msg.voltage,
        current: msg.current,
        vibration: msg.vibration,
        health: msg.health,
      };
      if (calState === 'collecting' || calState === 'warmup') {
        renderMotorCards();
      }
    } else if (msg.type === 'snapshot') {
      if (Array.isArray(msg.data)) {
        msg.data.forEach(function (d) {
          latestMotorData[d.id] = {
            temp: d.temp,
            voltage: d.voltage,
            current: d.current,
            vibration: d.vibration,
            health: d.health,
          };
        });
      }
    } else if (msg.type === 'calibration_status') {
      updateCalState(msg);
    } else if (msg.type === 'pong') {
      // keep alive
    }
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
      var remaining = Math.max(0, Math.round((status.warmup_remaining_s || 0) / 60));
      var mins = Math.floor(remaining);
      var secs = Math.max(0, Math.round((status.warmup_remaining_s || 0) % 60));
      var pct = status.warmup_pct || 0;

      warmupRow.style.display = 'flex';
      warmupFill.style.width = pct + '%';
      warmupText.textContent = mins + 'm ' + secs + 's remaining';
      collectRow.style.display = 'none';

      phaseLabel.textContent = 'Warming up — no data recorded';
      setStartPauseBtn(true);
      statusDot.className = 'status-dot dot-yellow pulse';
      statusLabel.textContent = 'Warming up ' + pct + '%';
    } else if (calState === 'collecting') {
      warmupRow.style.display = 'none';
      collectRow.style.display = 'flex';
      collectFill.style.width = Math.min(100, samplesCollected / 500) + '%';
      collectText.textContent = samplesCollected + ' samples';

      phaseLabel.textContent = 'Recording data — press Pause to stop';
      setStartPauseBtn(true);
      statusDot.className = 'status-dot dot-green pulse';
      statusLabel.textContent = 'Collecting';
    } else {
      // idle
      warmupRow.style.display = 'none';
      collectRow.style.display = 'none';
      phaseLabel.textContent = 'Press Start to begin';
      setStartPauseBtn(false);
      statusDot.className = 'status-dot dot-green';
      statusLabel.textContent = 'Ready';
    }

    samplesLabel.textContent = samplesCollected + ' samples collected';

    if (prevState !== calState) {
      renderMotorCards();
    }
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
      var statusClass = calState === 'idle' ? 'idle' : (calState === 'collecting' ? 'collecting' : 'warmup');
      var statusText = calState === 'idle' ? 'Idle' : (calState === 'collecting' ? 'Collecting' : 'Warming');

      var tempStr = d ? d.temp.toFixed(1) + ' \u00b0C' : '-- \u00b0C';
      var voltStr = d ? d.voltage.toFixed(2) + ' V' : '-- V';
      var currStr = d ? d.current.toFixed(3) + ' A' : '-- A';
      var vibStr = d ? (d.vibration ? 'YES' : 'no') : '--';
      var healthStr = d ? d.health.toFixed(1) + '%' : '--';

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
      if (d) {
        var hpct = Math.min(100, d.health || 100);
        html +=
          '<div style="margin-top:8px;height:3px;border-radius:9999px;background:var(--bg-white-05);overflow:hidden;">' +
          '<div style="height:100%;width:' + hpct +
          '%;border-radius:9999px;background:var(--primary);transition:width 0.5s ease;"></div></div>';
      }
      html += '</div>';
    });
    motorGrid.innerHTML = html;
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
    statusDot.className = 'status-dot dot-green';
    statusLabel.textContent = 'Ready';
    samplesLabel.textContent = '0 samples collected';
    calState = 'idle';
    samplesCollected = 0;
    latestMotorData = {};
    showLossBanner(false);
    renderMotorCards();
  }

  // ── Event listeners ───────────────────────────────────
  startBtn.addEventListener('click', toggleCalibration);
  downloadBtn.addEventListener('click', downloadData);
  resetBtn.addEventListener('click', resetCalibration);

  // Periodic UI refresh
  uiTimer = setInterval(pollStatus, 2000);

  // ── Init ──────────────────────────────────────────────
  renderMotorCards();
  connectWS();
})();
