(function() {
  'use strict';

  var motorNames = ['FL', 'FR', 'RL', 'RR'];
  var motors = motorNames.map(function(name, i) {
    return {
      id: i + 1,
      label: name,
      status: 'idle',
      samples: 0,
      avgTemp: 0,
      avgVoltage: 0,
      avgCurrent: 0,
      vibCount: 0
    };
  });

  var isCollecting = false;
  var elapsedHours = 0;
  var totalHours = 12;
  var collectionTimer = null;
  var motorGrid = document.getElementById('cal-motor-grid');
  var progressFill = document.getElementById('cal-progress-fill');
  var progressText = document.getElementById('cal-progress-text');
  var statusDot = document.getElementById('cal-status-dot');
  var statusLabel = document.getElementById('cal-status-label');
  var startBtn = document.getElementById('cal-start-btn');
  var resetBtn = document.getElementById('cal-reset-btn');

  function renderMotorCards() {
    var html = '';
    motors.forEach(function(m) {
      var statusClass = m.status === 'collecting' ? 'collecting' : (m.status === 'done' ? 'done' : 'idle');
      var statusText = m.status === 'collecting' ? 'Collecting' : (m.status === 'done' ? 'Done' : 'Idle');
      var sensorData = m.samples > 0 ? [
        { label: 'Temperature', value: (m.avgTemp / m.samples).toFixed(1) + ' °C' },
        { label: 'Voltage', value: (m.avgVoltage / m.samples).toFixed(2) + ' V' },
        { label: 'Current', value: (m.avgCurrent / m.samples).toFixed(3) + ' A' },
        { label: 'Vibration Events', value: m.vibCount }
      ] : [
        { label: 'Temperature', value: '-- °C' },
        { label: 'Voltage', value: '-- V' },
        { label: 'Current', value: '-- A' },
        { label: 'Samples', value: '0' }
      ];

      html += '<div class="cal-motor-card">';
      html += '<div class="cal-motor-header">';
      html += '<span class="cal-motor-name">Motor ' + m.label + '</span>';
      html += '<span class="cal-motor-status ' + statusClass + '">' + statusText + '</span>';
      html += '</div>';
      sensorData.forEach(function(s) {
        html += '<div class="cal-sensor-row">';
        html += '<span class="cal-sensor-label">' + s.label + '</span>';
        html += '<span class="cal-sensor-value">' + s.value + '</span>';
        html += '</div>';
      });
      html += '<div style="margin-top:8px;height:3px;border-radius:9999px;background:var(--bg-white-05);overflow:hidden;">';
      html += '<div style="height:100%;width:' + Math.min(100, (m.samples / 5000) * 100) + '%;border-radius:9999px;background:var(--primary);transition:width 0.5s ease;"></div>';
      html += '</div>';
      html += '</div>';
    });
    motorGrid.innerHTML = html;
  }

  function updateProgress() {
    var pct = Math.min(100, (elapsedHours / totalHours) * 100);
    progressFill.style.width = pct + '%';
    progressText.textContent = elapsedHours.toFixed(1) + ' / ' + totalHours + ' hours';
  }

  function simulateSensorReading(motor) {
    motor.samples++;
    motor.avgTemp += 35 + Math.random() * 15;
    motor.avgVoltage += 12 + (Math.random() - 0.5) * 0.8;
    motor.avgCurrent += 0.5 + Math.random() * 1.0;
    if (Math.random() < 0.05) motor.vibCount++;
  }

  function collectionTick() {
    elapsedHours += 0.1;
    motors.forEach(function(m) {
      if (elapsedHours < totalHours) {
        m.status = 'collecting';
        for (var i = 0; i < 10; i++) simulateSensorReading(m);
      } else {
        m.status = 'done';
      }
    });
    updateProgress();
    renderMotorCards();
    if (elapsedHours >= totalHours) {
      stopCollection();
    }
  }

  function startCollection() {
    if (isCollecting) return;
    isCollecting = true;
    motors.forEach(function(m) {
      m.status = 'collecting';
      m.samples = 0;
      m.avgTemp = 0;
      m.avgVoltage = 0;
      m.avgCurrent = 0;
      m.vibCount = 0;
    });
    elapsedHours = 0;
    statusDot.className = 'status-dot dot-yellow pulse';
    statusLabel.textContent = 'Collecting...';
    startBtn.querySelector('.btn-content').innerHTML = '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><rect x="6" y="4" width="4" height="16"/><rect x="14" y="4" width="4" height="16"/></svg> Running';
    collectionTimer = setInterval(collectionTick, 500);
    renderMotorCards();
  }

  function stopCollection() {
    isCollecting = false;
    clearInterval(collectionTimer);
    statusDot.className = 'status-dot dot-green pulse';
    statusLabel.textContent = 'Complete';
    startBtn.querySelector('.btn-content').innerHTML = '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg> Start';
    renderMotorCards();
  }

  function resetCollection() {
    clearInterval(collectionTimer);
    isCollecting = false;
    elapsedHours = 0;
    motors.forEach(function(m) {
      m.status = 'idle';
      m.samples = 0;
      m.avgTemp = 0;
      m.avgVoltage = 0;
      m.avgCurrent = 0;
      m.vibCount = 0;
    });
    statusDot.className = 'status-dot dot-green';
    statusLabel.textContent = 'Ready';
    startBtn.querySelector('.btn-content').innerHTML = '<svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg> Start';
    updateProgress();
    renderMotorCards();
  }

  startBtn.addEventListener('click', function() {
    if (isCollecting) return;
    startCollection();
  });

  resetBtn.addEventListener('click', resetCollection);

  renderMotorCards();
  updateProgress();
})();
