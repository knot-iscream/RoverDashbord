local M = {}

local LOW_FUEL_THRESHOLD = 0.10

local lowFuelMessageSent = false

local FUEL_ICON = "/ui/modules/apps/improvedVehicleDamageApp/fuelPump.svg"

local function findEngine()
  for _, device in pairs(powertrain.getDevices()) do
    if device.deviceCategories and device.deviceCategories.engine
       and device.registeredEnergyStorages then
      return device
    end
  end
end

local function updateGFX(dt)
  local device = findEngine()
  if not device then return end

  local total = 0
  local count = 0
  for _, s in pairs(device.registeredEnergyStorages) do
    local storage = energyStorage.getStorage(s)
    if storage and storage.energyType == device.requiredEnergyType
       and storage.remainingRatio ~= nil then
      total = total + storage.remainingRatio
      count = count + 1
    end
  end

  if count == 0 then return end

  local isLow = (total / count) < LOW_FUEL_THRESHOLD

  if isLow and not lowFuelMessageSent then
    lowFuelMessageSent = true
    local msg = device.requiredEnergyType == "electricEnergy" and "Low Battery level" or "Low Fuel level"
    guihooks.message(msg, 5, "vehicle.fuel.lowFuel", FUEL_ICON)
  elseif not isLow then
    lowFuelMessageSent = false
  end
end

local function reset()
  lowFuelMessageSent = false
end

M.updateGFX = updateGFX
M.reset      = reset
M.onReset    = reset

return M