local M = {}

-- Only guards the guihooks.message top-left notification.
-- damageTracker handles all DamageData state and event firing.
local blockMessageSent = false
local wallMessageSent  = false

local function findThermals()
  for _, device in pairs(powertrain.getDevices()) do
    if device.thermals then
      return device.thermals
    end
  end
end

local function updateGFX(dt)
  local t = findThermals()
  if not t then return end

  -- Engine block: trigger once damage starts accumulating
  if t.engineBlockOverheatDamage > 0 then
    if not blockMessageSent then
      blockMessageSent = true
    end
    damageTracker.setDamage("engine", "engineBlockDamage", true)
  else
    if blockMessageSent then
      blockMessageSent = false
    end
    damageTracker.setDamage("engine", "engineBlockDamage", false)
  end

  -- Cylinder wall: trigger once damage starts accumulating
  if t.cylinderWallOverheatDamage > 0 then
    if not wallMessageSent then
      wallMessageSent = true
    end
    damageTracker.setDamage("engine", "cylinderWallDamage", true)
  else
    if wallMessageSent then
      wallMessageSent = false
    end
    damageTracker.setDamage("engine", "cylinderWallDamage", false)
  end
end

local function reset()
  blockMessageSent = false
  wallMessageSent  = false
  damageTracker.setDamage("engine", "engineBlockDamage", false)
  damageTracker.setDamage("engine", "cylinderWallDamage", false)
end

M.updateGFX = updateGFX
M.reset = reset

return M