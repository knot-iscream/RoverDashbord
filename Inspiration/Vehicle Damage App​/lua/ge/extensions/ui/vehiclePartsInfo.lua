local M = {}

local engineDogTeeth = {
  ['nine_engine_i4_flathead_134'] = true,
  ['nine_engine_v8_flathead_232'] = true,
  ['nine_engine_v8_ohv_291']      = false,
  ['nine_engine_v8_ohv_353']      = false,
  ['nine_engine_v8_ohv_423']      = false,
}

local function getParts(vehicle, vehId)
  local pcFile = vehicle.partConfig
  if pcFile and pcFile ~= '' then
    local ok, pcData = pcall(jsonReadFile, pcFile)
    if ok and type(pcData) == 'table' and type(pcData.parts) == 'table' then
      return pcData.parts
    end
  end
  local details = core_vehicles.getVehicleDetails(vehId)
  if details then
    if details.configs and details.configs.parts then return details.configs.parts end
    if details.model  and details.model.parts   then return details.model.parts   end
  end
  return nil
end

local function sendPartsInfo(vehId)
  local vehicle = getObjectByID(vehId)
  if not vehicle then return end

  local modelKey = string.lower(vehicle.JBeam or '')
  local parts    = getParts(vehicle, vehId)

  -- Dog teeth: Nine only. pc file is reliable for engine selection.
  local hasDogTeeth = false
  if modelKey == 'nine' and parts then
    local enginePart = parts['nine_engine']
    if enginePart ~= nil and engineDogTeeth[enginePart] ~= nil then
      hasDogTeeth = engineDogTeeth[enginePart]
    end
  end

  -- Heavy dually: look for ANY installed part whose own internal name indicates it's a
  -- wheel/hub-related part AND indicates the heavy/dual-wheel variant. We deliberately do
  -- NOT require the slot key (e.g. 'pickup_hub_R') to contain 'hub' - some mods attach
  -- these through a completely different slot system (e.g. a 'wheel_converter' slot type),
  -- so the only thing we can reliably check is the chosen part's own name/value. This is a
  -- heuristic, not a guaranteed-correct check: it'll miss any part whose internal name
  -- doesn't reference wheels/hubs and heavy/dual/8-lug at all.
  -- CONFIRMED BUG (fixed below): 'wheel' as a bare substring also matches inside "flywheel"
  -- (an unrelated engine part, e.g. "us_semi_flywheel_heavy" on some T-series configs) -
  -- this caused false positives whenever a vehicle had ANY heavy flywheel installed,
  -- completely unrelated to its actual tires/wheels. Explicitly excluded below.
  -- Part changes are handled by VehicleConfigChange in JS (sets configChangedSinceReset),
  -- preventing this stale value from overwriting the correct live chosenPartsTree value.
  local hasHeavyDuallyWheels = false
  if parts then
    for slotKey, partValue in pairs(parts) do
      if type(slotKey) == 'string' and type(partValue) == 'string' then
        local lowerVal = string.lower(partValue)
        -- 'wheel' as a bare substring also matches inside unrelated words like
        -- "flywheel" (engine part, e.g. "us_semi_flywheel_heavy") - exclude that
        -- explicitly so an unrelated heavy flywheel can't falsely trigger this.
        local mentionsFlywheel = lowerVal:find('flywheel')
        local mentionsWheelOrHub = (lowerVal:find('wheel') and not mentionsFlywheel) or lowerVal:find('hub')
        local mentionsHeavyDually = lowerVal:find('heavy') or lowerVal:find('dual') or lowerVal:find('8[%s_-]?lug')
        if mentionsWheelOrHub and mentionsHeavyDually then
          hasHeavyDuallyWheels = true
        end
      end
    end
  end

  guihooks.trigger('VehiclePartsInfo', {
    vehId                = vehId,
    model                = modelKey,
    hasDogTeeth          = hasDogTeeth,
    hasHeavyDuallyWheels = hasHeavyDuallyWheels,
  })
end

-- Called from JS fetchCurrentVehicleModel() with a delay so the pc file
-- is guaranteed to be written before we read it.
local function sendCurrentPartsInfo()
  sendPartsInfo(be:getPlayerVehicleID(0))
end

local function onVehicleSwitched(oldId, newId, player)
  if newId and newId ~= -1 then
    sendPartsInfo(newId)
  end
end

M.sendCurrentPartsInfo = sendCurrentPartsInfo
M.onVehicleSwitched    = onVehicleSwitched

return M