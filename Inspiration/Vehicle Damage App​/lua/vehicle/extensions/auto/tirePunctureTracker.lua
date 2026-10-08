local M = {}

local lastPunctureState = {}

local function updateGFX(dt)
  if not wheels or not wheels.wheels then return end

  for i = 0, wheels.wheelCount - 1 do
    local wd = wheels.wheels[i]
    if wd then
      local wheelName  = wd.name
      local isPunctured = wd.isPunctured or false

      if lastPunctureState[wheelName] == nil then
        lastPunctureState[wheelName] = false
      end

      if isPunctured ~= lastPunctureState[wheelName] then
        lastPunctureState[wheelName] = isPunctured
        -- Use damageTracker.setDamage so events are part of BeamNG's complete
        -- DamageData snapshots rather than standalone partial events
        damageTracker.setDamage("wheels", "tirePunctured" .. wheelName, isPunctured)
      end
    end
  end
end

local function onReset()
  lastPunctureState = {}

  if wheels and wheels.wheels then
    for i = 0, wheels.wheelCount - 1 do
      local wd = wheels.wheels[i]
      if wd then
        damageTracker.setDamage("wheels", "tirePunctured" .. wd.name, false)
      end
    end
  end
end

M.updateGFX = updateGFX
M.onReset = onReset

return M