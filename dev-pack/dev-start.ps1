# RoverDash DEV launcher. Three jobs, all automatic:
#   1. add a Windows Firewall rule for the bundled MQTT broker (needs admin)
#   2. detect this PC's LAN IPv4 and write it into the three dev config.h files
#   3. start RoverDash
# The firmware still has to be flashed from Arduino IDE after this runs.

$ErrorActionPreference = 'Stop'
$devDir  = Split-Path -Parent $PSCommandPath
$appDir  = Split-Path -Parent $devDir
$appExe  = Join-Path $appDir 'roverdash.exe'
$broker  = Join-Path $appDir 'mosquitto-x86_64-pc-windows-msvc.exe'
$marker  = 'ROVER_DEV_BROKER'

# Re-launch elevated once. Everything below needs admin for the firewall rule.
$isAdmin = ([Security.Principal.WindowsPrincipal] `
  [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
  Write-Host 'Requesting administrator rights for the firewall rule...'
  $argLine = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $PSCommandPath + '"'))
  Start-Process -FilePath 'powershell.exe' -ArgumentList $argLine -Verb RunAs
  exit
}

# 1. Firewall: allow inbound MQTT to the broker bundled beside the app.
$ruleName = 'RoverDash DEV MQTT 1883'
$existing = Get-NetFirewallRule -DisplayName $ruleName -ErrorAction SilentlyContinue
if (-not $existing) {
  New-NetFirewallRule -DisplayName $ruleName -Direction Inbound -Protocol TCP `
    -LocalPort 1883 -Action Allow -Program $broker -Profile Any | Out-Null
  Write-Host 'Firewall rule added for the bundled broker on port 1883.'
} else {
  Write-Host 'Firewall rule already present.'
}

# 2. Find the active LAN address (the interface that owns the default route).
$route = Get-NetRoute -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue |
         Sort-Object RouteMetric | Select-Object -First 1
$pcIp = $null
if ($route) {
  $pcIp = (Get-NetIPAddress -AddressFamily IPv4 -InterfaceIndex $route.ifIndex `
           -ErrorAction SilentlyContinue |
           Where-Object { $_.IPAddress -notlike '127.*' -and $_.IPAddress -notlike '169.254.*' } |
           Select-Object -First 1).IPAddress
}
if (-not $pcIp) {
  $pcIp = (Get-NetIPAddress -AddressFamily IPv4 -ErrorAction SilentlyContinue |
           Where-Object { $_.IPAddress -notlike '127.*' -and $_.IPAddress -notlike '169.254.*' } |
           Select-Object -First 1).IPAddress
}
if (-not $pcIp) {
  Write-Host 'NO LAN IPv4 FOUND. Connect this PC to the rover WiFi and run again.'
  exit 1
}
Write-Host "This PC address for the boards: $pcIp"

# 3. Patch the broker line in each dev sketch. The marker makes this safe to
#    re-run after an IP change: always the line carrying ROVER_DEV_BROKER.
$patched = 0
Get-ChildItem -Path (Join-Path $devDir 'firmware') -Filter config.h -Recurse | ForEach-Object {
  $lines = [System.IO.File]::ReadAllLines($_.FullName)
  $changed = $false
  for ($i = 0; $i -lt $lines.Length; $i++) {
    if ($lines[$i] -like "*$marker*") {
      $lines[$i] = '#define MQTT_BROKER "' + $pcIp + '"  // ' + $marker
      $changed = $true
    }
    if ($lines[$i] -match '^\s*#define\s+SERIAL_BRIDGE\s+1') {
      $lines[$i] = '#define SERIAL_BRIDGE 0'
      $changed = $true
    }
  }
  if ($changed) {
    [System.IO.File]::WriteAllLines($_.FullName, $lines, (New-Object System.Text.UTF8Encoding($false)))
    $patched++
  }
}
if ($patched -eq 0) {
  Write-Host 'WARNING: no config.h files were patched. Did the DEV folder copy fail?'
} else {
  Write-Host "DEV config written into $patched sketches (WiFi path, SERIAL_BRIDGE 0)."
}

# 4. Start the app. Boards join the baked WiFi and land on the broker above.
if (Test-Path -LiteralPath $appExe) {
  Start-Process -FilePath $appExe -WorkingDirectory $appDir
  Write-Host 'RoverDash started.'
} else {
  Write-Host "Cannot find $appExe - run roverdash.exe from the RoverDash folder instead."
}
Write-Host 'Next: open the three sketches in DEV\firmware with Arduino IDE and flash them.'
