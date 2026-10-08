local M = {}

local clutchOverheatingActive = false

local function findClutch()
  for _, device in pairs(powertrain.getDevices()) do
    if device.deviceCategories and device.deviceCategories.clutch then
      return device
    end
  end
end

local function updateGFX(dt)
  local device = findClutch()
  if not device then return end

  local isOverheating = device.thermalEfficiency and device.thermalEfficiency < 1

  if isOverheating then
    damageTracker.setDamage("engine", "clutchOverheating", true)
  else
    damageTracker.setDamage("engine", "clutchOverheating", false)
  end

  clutchOverheatingActive = isOverheating or false
end

local function reset()
  clutchOverheatingActive = false
  damageTracker.setDamage("engine", "clutchOverheating", false)
end

M.updateGFX = updateGFX
M.reset = reset

return M