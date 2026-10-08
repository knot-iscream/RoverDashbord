(function () {
  'use strict'
  angular.module('beamng.apps')
  .directive('improvedVehicleDamageApp', ['$sce', '$timeout', 'translateService', function ($sce, $timeout, translateService) {
    return {
      template:
      `<div style="width:100%; height:100%; position:relative;">
          <div style="width:100%; height:100%;"
               ng-include="'/ui/modules/apps/improvedVehicleDamageApp/damage_car.svg'"
               onload="svgLoaded()"></div>
        </div>`,
      replace: true,
      link: function ($scope, element, attrs) {



        // Streams:
        var streamsList = ['wheelThermalData', 'engineInfo']
        StreamsManager.add(streamsList)

        var noDataColor = 'rgba(0,   0,   0,   0  )',
            greenColor  = 'rgba(0,   255, 0,   0.6)',
            orangeColor = 'rgba(255, 132, 0,   0.6)',
            redColor    = 'rgba(255, 0,   0,   0.6)',
            damageQueue = [],     // Array used to store each instance of damage so that damage text can be cycled through
            hasDamage = 0, // Value to check if damage has occurred
            newVisualDamage = false, // Flag to track if new visual damage occurred in current update
            textDisplayTime = 2000,// Amount of time damage text is shown in milliseconds
            beams = {},
            appDisplayed = 0,
            animTimeout,
            damageTimeout = null,
            textFunction,
            permanentDamage = 0,
            permanentDamagedParts = 0,
            svgElementPriorities = {},  // Track highest priority for each SVG element
            lastDamageData = null,
            configChangedSinceReset = false,
            svgCarGroup = null,  // Store reference to SVG car group
            clickableElements = {},  // Track which SVG elements have click handlers
            currentVehicleModel = null,  // JBeam name of the currently focused vehicle
            currentHasDogTeeth = false,  // true when the active gearbox uses dog teeth instead of synchros, set by VehiclePartsInfo event from ui_vehiclePartsInfo.lua
            currentHasHeavyDuallyWheels = false, // true when a heavy 8-lug dually hub/wheel part is installed, set by VehiclePartsInfo event from vehiclePartsInfo.lua
            currentVehId = null, // the actual BeamNG vehicle ID of the currently focused vehicle, from VehiclePartsInfo's data.vehId
            heavyDuallyConfirmedByVehId = loadHeavyDuallyCache(), // vehId+model key -> true, once confirmed by either source. Some vehicles/mods don't reliably re-surface the part on every Lua-side re-read; once seen for a specific vehicle instance, we trust it for the rest of the session rather than letting an occasional flaky read revert it to wrong. Persisted to sessionStorage so it also survives a UI reload, not just a vehicle switch.
            svgListeners = [],   // deregister fns — prevents duplicate handlers on ng-include re-render
            isInitialLoad = true // true on fresh F5 load; set false on any vehicle switch so sessionStorage restore is skipped

          // Persists heavyDuallyConfirmedByVehId across a UI reload (not just a vehicle switch) - a plain JS variable resets to {} on F5, which otherwise meant a
          // vehicle whose Lua-side read is flaky would revert to the wrong message the moment the UI reloads, even if it had already been correctly confirmed before.
          // Function declarations are hoisted, so calling this in the var block above (before its definition textually appears) is safe.
          function loadHeavyDuallyCache() {
            try {
              var stored = sessionStorage.getItem('improvedDmg_heavyDuallyConfirmedByVehId')
              return stored ? JSON.parse(stored) : {}
            } catch(e) {
              return {}
            }
          }

          function persistHeavyDuallyCache() {
            try {
              sessionStorage.setItem('improvedDmg_heavyDuallyConfirmedByVehId', JSON.stringify(heavyDuallyConfirmedByVehId))
            } catch(e) {}
          }

          // BeamNG reuses small integer vehId values once a vehicle is despawned, so a vehId alone isn't a stable identity for a specific vehicle instance across a whole session.
          // Combining vehId with the model name disambiguates that reuse case while still letting a single vehicle's own flaky re-reads be correctly remembered across a model-stable lifetime (vehId+model can't change without it being a different vehicle/spawn).
          function heavyDuallyCacheKey(vehId, model) {
            return vehId + ':' + (model || '')
          }


          // Static per-vehicle message overrides
          // These apply unconditionally whenever the named vehicle is focused
          var vehicleMessageOverrides = {

            // Hirochi SBR4
            'sbr': {
              'Rear Driveshaft Broken':           'Front Driveshaft Broken'
            },

            // Hirochi Aurata
            'utv': {
              'Rear Driveshaft Broken':           'Front Driveshaft Broken'
            },

            // Gavril T-Series
            'us_semi': {
              'Synchronizer Damage':              'Gear Dog Damage'
            },

            // Gavril MD-Series
            'md_series': {
              'Synchronizer Damage':              'Gear Dog Damage'
            }

          }

          // Conditional per-vehicle message overrides
          // Each entry is keyed by the vehicle model name and holds an array of rules
          var vehicleConditionalMessageOverrides = {

            // Bruckell Nine
            // "Gear Dog Damage" should only appear when the flathead engines are installed (which has no synchros, only dog teeth)
            // With the newer OHV engines, display "Synchronizer Damage"
            'nine': [
              {
                defaultText:  'Synchronizer Damage',
                overrideText: 'Gear Dog Damage',
                condition: function() {
                  return currentHasDogTeeth
                }
              }
            ]

          }

        // Method only called once the SVG has completed loaded
        $scope.svgLoaded = function () {
          for (var i = 0; i < svgListeners.length; i++) { svgListeners[i]() }
          svgListeners = []

          var svg = element[0].children[0].children[0]
          svgCarGroup = hu('#carGroup', svg)
          var svgDamageTextContainer = hu('#dmgContainer', svg),
              svgDamageText = hu('#dmgText', svg),
              defaultDamageTextFontSize = svgDamageText.n.style.fontSize || window.getComputedStyle(svgDamageText.n).fontSize,
              defaultDamageTextY = svgDamageText.n.getAttribute('y')

          // Fetch the model name for whatever vehicle is already focused
          fetchCurrentVehicleModel()



          // Map for powertrain components
          // Since we re-use the same SVG for multiple damage types, the priority value is used so
          // that more important damage can be shown as red else the damage will be shown as orange
          var componentDamageMap = {
            body: {
              FL: { svgId: '#bodyFL', priority: 2, damageDisplayed: 0, tempDamage: false },
              FR: { svgId: '#bodyFR', priority: 2, damageDisplayed: 0, tempDamage: false },
              ML: { svgId: '#bodyML', priority: 2, damageDisplayed: 0, tempDamage: false },
              MR: { svgId: '#bodyMR', priority: 2, damageDisplayed: 0, tempDamage: false },
              RL: { svgId: '#bodyRL', priority: 2, damageDisplayed: 0, tempDamage: false },
              RR: { svgId: '#bodyRR', priority: 2, damageDisplayed: 0, tempDamage: false }
            },

            engine: {
              coolantOverheating:             { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Coolant Overheating',             tempDamage: true, clickable: true },
              turbochargerHot:                { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Turbocharger Overheating',        tempDamage: true, clickable: true },
              clutchOverheating:              { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Clutch Overheating',              tempDamage: true, clickable: true },
              oilOverheating:                 { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Oil Overheating',                 tempDamage: true, clickable: true },
              starvedOfOil:                   { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Oil Starvation Damage',           tempDamage: true, clickable: true },
              oilLevelCritical:               { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Low Oil Pressure',                tempDamage: false },
              oilLevelTooHigh:                { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'High Oil Pressure',               tempDamage: false },
              pistonRingsDamaged:             { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Piston Rings Damaged',            tempDamage: false },
              rodBearingsDamaged:             { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Rod Bearings Damaged',            tempDamage: false },
              headGasketDamaged:              { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Head Gasket Blown',               tempDamage: false },
              engineIsHydrolocking:           { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Engine is Hydrolocking',          tempDamage: true, clickable: true },
              engineReducedTorque:            { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Engine Torque Reduced',           tempDamage: false },
              mildOverrevDamage:              { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Mild Over Rev Damage',            tempDamage: false },
              catastrophicOverrevDamage:      { svgId: '#engine',     priority: 1, damageDisplayed: 0, damageText: 'Catastrophic Over Rev Damage',    tempDamage: false },
              overRevDanger:                  { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Over Rev Danger',                 tempDamage: false },
              overTorqueDanger:               { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Over Torque Danger',              tempDamage: false },
              catastrophicOverTorqueDamage:   { svgId: '#engine',     priority: 1, damageDisplayed: 0, damageText: 'Catastrophic Over Torque Damage', tempDamage: false },
              engineHydrolocked:              { svgId: '#engine',     priority: 1, damageDisplayed: 0, damageText: 'Engine is Hydrolocked',           tempDamage: false },
              engineDisabled:                 { svgId: '#engine',     priority: 1, damageDisplayed: 0, damageText: 'Engine Disabled',                 tempDamage: false },
              blockMelted:                    { svgId: '#engine',     priority: 1, damageDisplayed: 0, damageText: 'Engine Block Melted',             tempDamage: false },
              cylinderWallsMelted:            { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Cylinder Walls Melted',           tempDamage: false },
              engineLockedUp:                 { svgId: '#engine',     priority: 1, damageDisplayed: 0, damageText: 'Engine Locked Up',                tempDamage: false },
              radiatorLeak:                   { svgId: '#radiator',   priority: 1, damageDisplayed: 0, damageText: 'Radiator Leaking',                tempDamage: false },
              oilRadiatorLeak:                { svgId: '#radiator',   priority: 1, damageDisplayed: 0, damageText: 'Oil Radiator Leaking',            tempDamage: false },
              oilpanLeak:                     { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Oil Pan Leaking',                 tempDamage: false },
              inductionSystemDamaged:         { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Induction System Damage',         tempDamage: false },
              impactDamage:                   { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Engine Impact Damage',            tempDamage: false },
              engineBlockDamage:              { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Engine Block Damage',             tempDamage: false },
              cylinderWallDamage:             { svgId: '#engine',     priority: 0, damageDisplayed: 0, damageText: 'Cylinder Wall Damage',            tempDamage: false }
            },
            gearbox: {
              synchroWear: { svgId: '#engine', priority: 0, damageDisplayed: 0, damageText: 'Synchronizer Damage', tempDamage: true, clickable: true }
            },
            clutch: {},
            powertrain: {
              wheelaxleFL:       { svgId: '#wheelaxleFL',    priority: 1, damageDisplayed: 0, damageText: 'Front Left Axle Broken',          tempDamage: false },
              wheelaxle_FL:      { svgId: '#wheelaxleFL',    priority: 1, damageDisplayed: 0, damageText: 'Front Left Axle Broken',          tempDamage: false },
              wheelaxleFR:       { svgId: '#wheelaxleFR',    priority: 1, damageDisplayed: 0, damageText: 'Front Right Axle Broken',         tempDamage: false },
              wheelaxle_FR:      { svgId: '#wheelaxleFR',    priority: 1, damageDisplayed: 0, damageText: 'Front Right Axle Broken',         tempDamage: false },
              wheelaxleRL:       { svgId: '#wheelaxleRL',    priority: 1, damageDisplayed: 0, damageText: 'Rear Left Axle Broken',           tempDamage: false },
              wheelaxle_RL1:     { svgId: '#wheelaxleRL',    priority: 1, damageDisplayed: 0, damageText: 'Rear Left Axle Broken',           tempDamage: false },
              wheelaxleRR:       { svgId: '#wheelaxleRR',    priority: 1, damageDisplayed: 0, damageText: 'Rear Right Axle Broken',          tempDamage: false },
              wheelaxle_RR1:     { svgId: '#wheelaxleRR',    priority: 1, damageDisplayed: 0, damageText: 'Rear Right Axle Broken',          tempDamage: false },
              wheelaxle_1RL1:    { svgId: '#wheelaxleRL',    priority: 1, damageDisplayed: 0, damageText: 'Rear Left Hub Broken',            tempDamage: false },
              wheelaxle_1RR1:    { svgId: '#wheelaxleRR',    priority: 1, damageDisplayed: 0, damageText: 'Rear Right Hub Broken',           tempDamage: false },
              axle_1RL:          { svgId: '#wheelaxleRL',    priority: 1, damageDisplayed: 0, damageText: 'Rear Left Axle Broken',           tempDamage: false },
              axle_1RR:          { svgId: '#wheelaxleRR',    priority: 1, damageDisplayed: 0, damageText: 'Rear Right Axle Broken',          tempDamage: false },
              halfshaftFL:       { svgId: '#wheelaxleFL',    priority: 1, damageDisplayed: 0, damageText: 'Front Left Axle Broken',          tempDamage: false },
              halfshaftFR:       { svgId: '#wheelaxleFR',    priority: 1, damageDisplayed: 0, damageText: 'Front Right Axle Broken',         tempDamage: false },
              spindleFL:         { svgId: '#wheelaxleFL',    priority: 1, damageDisplayed: 0, damageText: 'Front Left Spindle Broken',       tempDamage: false },
              spindleFR:         { svgId: '#wheelaxleFR',    priority: 1, damageDisplayed: 0, damageText: 'Front Right Spindle Broken',      tempDamage: false },
              spindleRL:         { svgId: '#wheelaxleRL',    priority: 1, damageDisplayed: 0, damageText: 'Rear Left Spindle Broken',        tempDamage: false },
              spindleRR:         { svgId: '#wheelaxleRR',    priority: 1, damageDisplayed: 0, damageText: 'Rear Right Spindle Broken',       tempDamage: false },
              driveshaft:        { svgId: '#driveshaft',     priority: 1, damageDisplayed: 0, damageText: 'Rear Driveshaft Broken',          tempDamage: false },
              driveshaft_R:      { svgId: '#driveshaft',     priority: 1, damageDisplayed: 0, damageText: 'Rear Driveshaft Broken',          tempDamage: false },
              driveshaft2:       { svgId: '#driveshaft',     priority: 1, damageDisplayed: 0, damageText: 'Middle Driveshaft Broken',        tempDamage: false },
              driveshaft_F:      { svgId: '#driveshaft',     priority: 1, damageDisplayed: 0, damageText: 'Front Driveshaft Broken',         tempDamage: false },
              driveshaft_F_1:    { svgId: '#driveshaft',     priority: 1, damageDisplayed: 0, damageText: 'Front Front Driveshaft Broken',   tempDamage: false },
              driveshaft_F_2:    { svgId: '#driveshaft',     priority: 1, damageDisplayed: 0, damageText: 'Front Rear Driveshaft Broken',    tempDamage: false },
              driveshaft_left:   { svgId: '#driveshaft',     priority: 1, damageDisplayed: 0, damageText: 'Left Driveshaft Broken',          tempDamage: false },
              driveshaft_right:  { svgId: '#driveshaft',     priority: 1, damageDisplayed: 0, damageText: 'Right Driveshaft Broken',         tempDamage: false },
              propshaft:         { svgId: '#driveshaft',     priority: 1, damageDisplayed: 0, damageText: 'Propshaft Broken',                tempDamage: false },
              intershaft:        { svgId: '#driveshaft',     priority: 1, damageDisplayed: 0, damageText: 'Intershaft Broken',               tempDamage: false },
              intershaft1:       { svgId: '#driveshaft',     priority: 1, damageDisplayed: 0, damageText: 'Rear Intershaft Broken',          tempDamage: false },
              tcasRangeboxShaft: { svgId: '#driveshaft',     priority: 1, damageDisplayed: 0, damageText: 'Transfer Case Driveshaft Broken', tempDamage: false },
              mainEngine:        { svgId: '#engine',         priority: 1, damageDisplayed: 0, damageText: 'Main Engine Broken',              tempDamage: false },
              frontMotor:        { svgId: '#engine',         priority: 1, damageDisplayed: 0, damageText: 'Front Motor Broken',              tempDamage: false },
              rearMotor:         { svgId: '#engine',         priority: 1, damageDisplayed: 0, damageText: 'Rear Motor Broken',               tempDamage: false }
            },
            energyStorage: {
              mainTank:    { svgId: '#fueltank', priority: 1, damageDisplayed: 0, damageText: 'Fuel Tank Damaged',       tempDamage: false },
              cabTank:     { svgId: '#fueltank', priority: 1, damageDisplayed: 0, damageText: 'Fuel Tank Damaged',       tempDamage: false },
              mainTankL:   { svgId: '#fueltank', priority: 1, damageDisplayed: 0, damageText: 'Left Fuel Tank Damaged',  tempDamage: false },
              mainTankR:   { svgId: '#fueltank', priority: 1, damageDisplayed: 0, damageText: 'Right Fuel Tank Damaged', tempDamage: false },
              mainTank_L:  { svgId: '#fueltank', priority: 1, damageDisplayed: 0, damageText: 'Left Fuel Tank Damaged',  tempDamage: false },
              mainTank_R:  { svgId: '#fueltank', priority: 1, damageDisplayed: 0, damageText: 'Right Fuel Tank Damaged', tempDamage: false },
              fueltank_L:  { svgId: '#fueltank', priority: 1, damageDisplayed: 0, damageText: 'Left Fuel Tank Damaged',  tempDamage: false },
              fueltank_R:  { svgId: '#fueltank', priority: 1, damageDisplayed: 0, damageText: 'Right Fuel Tank Damaged', tempDamage: false },
              auxTank:     { svgId: '#fueltank', priority: 1, damageDisplayed: 0, damageText: 'Fuel Cell Damaged',       tempDamage: false },
              auxTank2:    { svgId: '#fueltank', priority: 1, damageDisplayed: 0, damageText: 'Fuel Cell Damaged',       tempDamage: false },
              frontTank:   { svgId: '#fueltank', priority: 1, damageDisplayed: 0, damageText: 'Fuel Cell Damaged',       tempDamage: false },
              rearTank:    { svgId: '#fueltank', priority: 1, damageDisplayed: 0, damageText: 'Fuel Cell Damaged',       tempDamage: false },
              mainAirTank: { svgId: '#fueltank', priority: 1, damageDisplayed: 0, damageText: 'Pressure Tank Damaged',   tempDamage: false }
            },
            wheels: {
              tireFL:             { svgId: '#tireFL',     priority: 0, damageDisplayed: 0, damageText: 'Front Left Tire Flat',            tempDamage: false },
              tireFR:             { svgId: '#tireFR',     priority: 0, damageDisplayed: 0, damageText: 'Front Right Tire Flat',           tempDamage: false },
              tireRL:             { svgId: '#tireRL',     priority: 0, damageDisplayed: 0, damageText: 'Rear Left Tire Flat',             tempDamage: false },
              tireRR:             { svgId: '#tireRR',     priority: 0, damageDisplayed: 0, damageText: 'Rear Right Tire Flat',            tempDamage: false },
              tireRL1:            { svgId: '#tireRL',     priority: 0, damageDisplayed: 0, damageText: 'Rear Left Outer Tire Flat',       tempDamage: false },
              tireRL2:            { svgId: '#tireRL',     priority: 0, damageDisplayed: 0, damageText: 'Rear Left Inner Tire Flat',       tempDamage: false },
              tireRR1:            { svgId: '#tireRR',     priority: 0, damageDisplayed: 0, damageText: 'Rear Right Outer Tire Flat',      tempDamage: false },
              tireRR2:            { svgId: '#tireRR',     priority: 0, damageDisplayed: 0, damageText: 'Rear Right Inner Tire Flat',      tempDamage: false },
              tireR1LL:           { svgId: '#tireRL',     priority: 0, damageDisplayed: 0, damageText: 'Rear Left Outer Tire Flat',       tempDamage: false },
              tireR1L:            { svgId: '#tireRL',     priority: 0, damageDisplayed: 0, damageText: 'Rear Left Inner Tire Flat',       tempDamage: false },
              tireR1RR:           { svgId: '#tireRR',     priority: 0, damageDisplayed: 0, damageText: 'Rear Right Outer Tire Flat',      tempDamage: false },
              tireR1R:            { svgId: '#tireRR',     priority: 0, damageDisplayed: 0, damageText: 'Rear Right Inner Tire Flat',      tempDamage: false },
              tirePuncturedFL:    { svgId: '#tireFL',     priority: 0, damageDisplayed: 0, damageText: 'Front Left Tire Punctured',       tempDamage: false },
              tirePuncturedFR:    { svgId: '#tireFR',     priority: 0, damageDisplayed: 0, damageText: 'Front Right Tire Punctured',      tempDamage: false },
              tirePuncturedRL:    { svgId: '#tireRL',     priority: 0, damageDisplayed: 0, damageText: 'Rear Left Tire Punctured',        tempDamage: false },
              tirePuncturedRR:    { svgId: '#tireRR',     priority: 0, damageDisplayed: 0, damageText: 'Rear Right Tire Punctured',       tempDamage: false },
              tirePuncturedRL1:   { svgId: '#tireRL',     priority: 0, damageDisplayed: 0, damageText: 'Rear Left Outer Tire Punctured',  tempDamage: false },
              tirePuncturedRL2:   { svgId: '#tireRL',     priority: 0, damageDisplayed: 0, damageText: 'Rear Left Inner Tire Punctured',  tempDamage: false },
              tirePuncturedRR1:   { svgId: '#tireRR',     priority: 0, damageDisplayed: 0, damageText: 'Rear Right Outer Tire Punctured', tempDamage: false },
              tirePuncturedRR2:   { svgId: '#tireRR',     priority: 0, damageDisplayed: 0, damageText: 'Rear Right Inner Tire Punctured', tempDamage: false },
              tirePuncturedR1LL:  { svgId: '#tireRL',     priority: 0, damageDisplayed: 0, damageText: 'Rear Left Outer Tire Punctured',  tempDamage: false },
              tirePuncturedR1L:   { svgId: '#tireRL',     priority: 0, damageDisplayed: 0, damageText: 'Rear Left Inner Tire Punctured',  tempDamage: false },
              tirePuncturedR1RR:  { svgId: '#tireRR',     priority: 0, damageDisplayed: 0, damageText: 'Rear Right Outer Tire Punctured', tempDamage: false },
              tirePuncturedR1R:   { svgId: '#tireRR',     priority: 0, damageDisplayed: 0, damageText: 'Rear Right Inner Tire Punctured', tempDamage: false },
              brakeFL:            { svgId: '#brakeFL',    priority: 1, damageDisplayed: 0, damageText: 'Front Left Brake Damaged',        tempDamage: false },
              brakeFR:            { svgId: '#brakeFR',    priority: 1, damageDisplayed: 0, damageText: 'Front Right Brake Damaged',       tempDamage: false },
              brakeRL:            { svgId: '#brakeRL',    priority: 1, damageDisplayed: 0, damageText: 'Rear Left Brake Damaged',         tempDamage: false },
              brakeRR:            { svgId: '#brakeRR',    priority: 1, damageDisplayed: 0, damageText: 'Rear Right Brake Damaged',        tempDamage: false },
              brakeRL1:           { svgId: '#brakeRL',    priority: 1, damageDisplayed: 0, damageText: 'Rear Left Brake Damaged',         tempDamage: false },
              brakeRR1:           { svgId: '#brakeRR',    priority: 1, damageDisplayed: 0, damageText: 'Rear Right Brake Damaged',        tempDamage: false },
              brakeR1L:           { svgId: '#brakeRL',    priority: 1, damageDisplayed: 0, damageText: 'Rear Left Brake Damaged',         tempDamage: false },
              brakeR1R:           { svgId: '#brakeRR',    priority: 1, damageDisplayed: 0, damageText: 'Rear Right Brake Damaged',        tempDamage: false },
              brakeOverHeatFL:    { svgId: '#brakeFL',    priority: 0, damageDisplayed: 0, damageText: 'Front Left Brake Fading',         tempDamage: true, clickable: true },
              brakeOverHeatFR:    { svgId: '#brakeFR',    priority: 0, damageDisplayed: 0, damageText: 'Front Right Brake Fading',        tempDamage: true, clickable: true },
              brakeOverHeatRL:    { svgId: '#brakeRL',    priority: 0, damageDisplayed: 0, damageText: 'Rear Left Brake Fading',          tempDamage: true, clickable: true },
              brakeOverHeatRR:    { svgId: '#brakeRR',    priority: 0, damageDisplayed: 0, damageText: 'Rear Right Brake Fading',         tempDamage: true, clickable: true },
              brakeOverHeatRL1:   { svgId: '#brakeRL',    priority: 0, damageDisplayed: 0, damageText: 'Rear Left Brake Fading',          tempDamage: true, clickable: true },
              brakeOverHeatRR1:   { svgId: '#brakeRR',    priority: 0, damageDisplayed: 0, damageText: 'Rear Right Brake Fading',         tempDamage: true, clickable: true },
              brakeOverHeatR1L:   { svgId: '#brakeRL',    priority: 0, damageDisplayed: 0, damageText: 'Rear Left Brake Fading',          tempDamage: true, clickable: true },
              brakeOverHeatR1R:   { svgId: '#brakeRR',    priority: 0, damageDisplayed: 0, damageText: 'Rear Right Brake Fading',         tempDamage: true, clickable: true },
              FL:                 { svgId: '#tireFL',     priority: 1, damageDisplayed: 0, damageText: 'Front Left Wheel Detached',       tempDamage: false },
              FR:                 { svgId: '#tireFR',     priority: 1, damageDisplayed: 0, damageText: 'Front Right Wheel Detached',      tempDamage: false },
              RL:                 { svgId: '#tireRL',     priority: 1, damageDisplayed: 0, damageText: 'Rear Left Wheel Detached',        tempDamage: false },
              RL1:                { svgId: '#tireRL',     priority: 1, damageDisplayed: 0, damageText: 'Rear Left Wheel Detached',        tempDamage: false },
              R1L:                { svgId: '#tireRL',     priority: 1, damageDisplayed: 0, damageText: 'Rear Left Wheel Detached',        tempDamage: false },
              RR:                 { svgId: '#tireRR',     priority: 1, damageDisplayed: 0, damageText: 'Rear Right Wheel Detached',       tempDamage: false },
              RR1:                { svgId: '#tireRR',     priority: 1, damageDisplayed: 0, damageText: 'Rear Right Wheel Detached',       tempDamage: false },
              R1R:                { svgId: '#tireRR',     priority: 1, damageDisplayed: 0, damageText: 'Rear Right Wheel Detached',       tempDamage: false }
            }
          }


          // Settings helpers (SVG-aware, defined after componentDamageMap)
          // Returns the idle/undamaged fill color based on the current theme setting
          function getIdleColor() {
            return noDataColor
          }

          // Initialize all SVG elements to the current idle color
          function initializeSvgElements() {
            var idleColor = getIdleColor()
            for (var key in componentDamageMap) {
              for (var val in componentDamageMap[key]) {
                hu(componentDamageMap[key][val].svgId, svg).css({ fill: idleColor })
              }
            }
          }

          // Apply all current settings to live SVG state
          // Safe to call at any time after SVG is loaded
          function applySettings() {
            var seenIds = {}
            for (var key in componentDamageMap) {
              for (var val in componentDamageMap[key]) {
                var svgId = componentDamageMap[key][val].svgId
                if (!seenIds[svgId]) {
                  seenIds[svgId] = true
                  updateSvgElement(svgId)
                }
              }
            }
            // Auto-hide if nothing is currently damaged
            if (permanentDamagedParts === 0 && !hasActiveTempDamage()) {
              $timeout.cancel(damageTimeout)
              damageTimeout = $timeout(function () {
                svgCarGroup.animate({ opacity: 0 }, 200)
              }, 2500)
            }
          }

          initializeSvgElements()
          applySettings()

          if (lastDamageData && Object.keys(lastDamageData).length > 0) {
            refreshDamage()
          }

          // Color helpers
          // Function to get the color based on highest priority for an SVG element
          function getColorForPriority(priority) {
            if (priority === 1) return redColor
            if (priority === 0) return orangeColor
            return getIdleColor()
          }

          // Function to update SVG element based on highest priority damage
          function updateSvgElement(svgId) {
            var highestPriority = svgElementPriorities[svgId]
            if (highestPriority !== undefined) {
              hu(svgId, svg).css({ fill: getColorForPriority(highestPriority) })
            } else {
              hu(svgId, svg).css({ fill: getIdleColor() })
            }
          }

          // Helper function to check if an SVG element has permanent (priority 1) damage
          function hasPermanentDamage(svgId) {
            return svgElementPriorities[svgId] === 1
          }

          // Helper function to check if any temporary damage is currently displayed
          function hasActiveTempDamage() {
            for (var key in componentDamageMap) {
              for (var val in componentDamageMap[key]) {
                var component = componentDamageMap[key][val]
                if (component.tempDamage && component.damageDisplayed === 1) {
                  return true
                }
              }
            }
            return false
          }

          // Damage text display
          var damageFontScales = {
            'Catastrophic Over Rev Damage':    0.85,
            'Catastrophic Over Torque Damage': 0.80
          }

          var damageYNudge = {
            'Catastrophic Over Rev Damage':    1.5,
            'Catastrophic Over Torque Damage': 1
          }

          // Translates a damageText string using damageTranslation, if a custom translation is available for the current UI language.
          // Checks the full locale first (e.g. 'pt_br') so region variants like pt_PT vs pt_BR can have different translations,
          // then falls back to the base language (e.g. 'pt') if no region-specific entry exists, and finally to BeamNG's own translateService.
          function localizeDamageText(text) {
            var locale = currentUiLocale()  // e.g. 'pt_br'
            var base = locale.split('_')[0] // e.g. 'pt'
            var table = damageTranslation[locale] || damageTranslation[base]
            var translated = table && table[text]
            if (translated) return translated
            return translateService.contextTranslate(text)
          }

          function showText() {
            if (damageQueue && damageQueue.length > 0) {
              var currentDamageText = damageQueue[0].damageText
              var scale = damageFontScales[currentDamageText]
              if (scale) {
                var fontSize = parseFloat(defaultDamageTextFontSize)
                var yOffset = (fontSize - (fontSize * scale)) / 2
                var nudge = damageYNudge[currentDamageText] || 0
                var svgRect = svg.getBoundingClientRect()
                var viewBox = svg.viewBox.baseVal
                var svgUnitsPerPx = viewBox.height / svgRect.height
                var totalOffsetInSVGUnits = (-yOffset + nudge) * svgUnitsPerPx
                svgDamageText.n.style.fontSize = 'calc(' + defaultDamageTextFontSize + ' * ' + scale + ')'
                svgDamageText.n.setAttribute('y', defaultDamageTextY)
                svgDamageText.n.setAttribute('transform', 'translate(0, ' + totalOffsetInSVGUnits + ')')
              } else {
                svgDamageText.n.style.fontSize = defaultDamageTextFontSize
                svgDamageText.n.setAttribute('y', defaultDamageTextY)
                svgDamageText.n.setAttribute('transform', '')
              }
              svgDamageText.css({opacity:1}).text(localizeDamageText(currentDamageText))
              svgDamageTextContainer.css({opacity:1})
              damageQueue.splice(0, 1)  // removing current item from array
              animTimeout = $timeout(showText, textDisplayTime)
            }
            else {
              svgDamageText.css({opacity:0}).text('')
              svgDamageTextContainer.css({opacity:0})

              if (permanentDamagedParts === 0 && !hasActiveTempDamage()) {
                if (damageTimeout) {
                  $timeout.cancel(damageTimeout)
                }
                damageTimeout = $timeout(function() {
                  svgCarGroup.animate({opacity: 0}, 200)
                }, 2500)
              } else {
                $timeout.cancel(damageTimeout)
              }

              appDisplayed = 0
              damageQueue = []
              $timeout.cancel(animTimeout)
            }
          }

          function showApp(arr) {
            svgCarGroup.animate({opacity: 1}, 200)
            showText()
          }

          // Reset
          function reset() {
            // Remove all click handlers
            for (var svgId in clickableElements) {
              removeClickFromElement(svgId)
            }

            // Reset all SVG elements to the current idle color and clear animations
            var idleColor = getIdleColor()
            for (var key in componentDamageMap) {
              for (var val in componentDamageMap[key]) {
                hu(componentDamageMap[key][val].svgId, svg).css({ fill: idleColor })
                componentDamageMap[key][val].damageDisplayed = 0
                hu(componentDamageMap[key][val].svgId, svg).n.classList.remove("flashAnim")
              }
            }
            // Reset priority tracking
            svgElementPriorities = {}

            // Reset clutch stream edge-trigger state
            hasDamage = 0
            newVisualDamage = false
            permanentDamage = 0
            permanentDamagedParts = 0
            appDisplayed = 0
            damageQueue = []
            showApp()
          }

          // Function to add click handler to a damaged element
          function addClickToDamagedElement(svgId) {
            if (!clickableElements[svgId]) {
              var node = hu(svgId, svg).n

              var clickHandler = function(event) {
                event.preventDefault()
                event.stopPropagation()
                refreshDamage()
              }
              var mouseoverHandler = function() {
                var isRed = svgElementPriorities[svgId] === 1
                var brightness = isRed ? 'brightness(1.6)' : 'brightness(1.2)'
                node.style.filter = brightness + ' drop-shadow(0 0 4px rgba(255,255,255,0.6))'
              }
              var mouseoutHandler = function() {
                node.style.filter = ''
              }

              node.addEventListener('click',     clickHandler)
              node.addEventListener('mouseover', mouseoverHandler)
              node.addEventListener('mouseout',  mouseoutHandler)
              node.style.cursor = 'pointer'

              // Store the handler references so removeClickFromElement can remove the exact same functions
              clickableElements[svgId] = {
                click:     clickHandler,
                mouseover: mouseoverHandler,
                mouseout:  mouseoutHandler
              }
            }
          }

          // Function to remove click handler from an element
          function removeClickFromElement(svgId) {
            var handlers = clickableElements[svgId]
            if (handlers) {
              var node = hu(svgId, svg).n
              node.removeEventListener('click',     handlers.click)
              node.removeEventListener('mouseover', handlers.mouseover)
              node.removeEventListener('mouseout',  handlers.mouseout)
              node.style.filter = ''
              node.style.cursor = 'default'
              delete clickableElements[svgId]
            }
          }

          // Function to update clickable elements based on current damage
          function updateClickableElements() {
            // Check which elements should be clickable
            var shouldBeClickable = {}

            for (var key in componentDamageMap) {
              for (var val in componentDamageMap[key]) {
                var component = componentDamageMap[key][val]
                if (component.damageDisplayed === 1 && (!component.tempDamage || component.clickable)) {
                  shouldBeClickable[component.svgId] = true
                }
              }
            }

            // Add click handlers to elements that should be clickable but aren't yet
            for (var svgId in shouldBeClickable) {
              addClickToDamagedElement(svgId)
            }

            // Remove click handlers from elements that shouldn't be clickable anymore
            for (var svgId in clickableElements) {
              if (!shouldBeClickable[svgId]) {
                removeClickFromElement(svgId)
              }
            }
          }

          // Refresh
          function refreshDamage() {
            if (!lastDamageData || Object.keys(lastDamageData).length === 0) {
              return // No damage data to refresh
            }

            $timeout.cancel(animTimeout)
            $timeout.cancel(damageTimeout)

            damageQueue = []
            appDisplayed = 0
            permanentDamage = 0
            permanentDamagedParts = 0
            svgElementPriorities = {}
            newVisualDamage = false
            hasDamage = 0

            for (var key in componentDamageMap) {
              for (var val in componentDamageMap[key]) {
                componentDamageMap[key][val].damageDisplayed = 0
              }
            }

            initializeSvgElements()

            for (var key in lastDamageData) {
              for (var val in lastDamageData[key]) {
                checkDamage(key, val, lastDamageData)
              }
            }

            updateClickableElements()

            if (damageQueue.length > 0 || newVisualDamage) {
              appDisplayed = 1
              showApp(damageQueue)
            }
          }

          // Set damage visual
          function setDamage(component, color, anim) {
            var node = hu(component.svgId, svg).css({ fill: color }).attr({ class: anim }).n
            if (anim === 'flashAnim') {
              // Use a one-time listener so repeated damage hits don't stack handlers
              function onAnimEnd() {
                node.classList.remove('flashAnim')
                node.removeEventListener('webkitAnimationEnd', onAnimEnd)
                node.removeEventListener('animationend',       onAnimEnd)
              }
              node.addEventListener('webkitAnimationEnd', onAnimEnd)
              node.addEventListener('animationend',       onAnimEnd)
            }
          }

          // Asks the ui_vehiclePartsInfo GE Lua extension to immediately query the current vehicle's gearbox and push a VehiclePartsInfo event
          // This is only needed on initial SVG load; all subsequent spawn and focus changes are handled automatically by the extension's onVehicleSpawned and onVehicleSwitched hooks
          function fetchCurrentVehicleModel() {
            bngApi.engineLua('extensions.ui_vehiclePartsInfo.sendCurrentPartsInfo()')
          }

          // Returns the override message for the current vehicle, or the default
          // Applied for any vehicle when the heavy dually hub is installed, regardless of model.
          // Mirrors the per-model rules in vehicleConditionalMessageOverrides but without the
          // model restriction - covers mods and any future vehicles that use the same hub part.
          var heavyDuallyUniversalRules = [
            { defaultText: 'Rear Left Inner Tire Flat',       overrideText: 'Rear Left Outer Tire Flat',       condition: function() { return currentHasHeavyDuallyWheels } },
            { defaultText: 'Rear Right Inner Tire Flat',      overrideText: 'Rear Right Outer Tire Flat',      condition: function() { return currentHasHeavyDuallyWheels } },
            { defaultText: 'Rear Left Inner Tire Punctured',  overrideText: 'Rear Left Outer Tire Punctured',  condition: function() { return currentHasHeavyDuallyWheels } },
            { defaultText: 'Rear Right Inner Tire Punctured', overrideText: 'Rear Right Outer Tire Punctured', condition: function() { return currentHasHeavyDuallyWheels } }
          ]

          function getVehicleDamageText(defaultText) {
            if (!defaultText || !currentVehicleModel) return defaultText

            // 1. Check conditional overrides first
            var ruleSetsToCheck = [heavyDuallyUniversalRules]
            if (currentVehicleModel && vehicleConditionalMessageOverrides[currentVehicleModel]) {
              ruleSetsToCheck.push(vehicleConditionalMessageOverrides[currentVehicleModel])
            }
            for (var rs = 0; rs < ruleSetsToCheck.length; rs++) {
              var conditionalRules = ruleSetsToCheck[rs]
              for (var i = 0; i < conditionalRules.length; i++) {
                var rule = conditionalRules[i]
                if (rule.defaultText === defaultText && rule.condition()) {
                  return rule.overrideText
                }
              }
            }

            // 2. Fall back to unconditional static overrides
            var overrides = vehicleMessageOverrides[currentVehicleModel]
            if (overrides && overrides[defaultText]) return overrides[defaultText]

            // 3. No override matched, return the original text
            return defaultText
          }

          // Core damage logic
          function checkDamage(type, component, data) {
            // Ignore clutch damage if we're in the post-reset cooldown period
            if (componentDamageMap[type] && componentDamageMap[type][component] !== undefined) {
              var damagedComponent = componentDamageMap[type][component]

              if (damagedComponent.damageDisplayed === 0) {
                if (data[type][component] === true || data[type][component] > 0) {
                  // Skip temp damage flash animation if permanent damage already exists
                  if (damagedComponent.tempDamage && hasPermanentDamage(damagedComponent.svgId)) {
                    // Mark as displayed but don't update visual or add to queue
                    damagedComponent.damageDisplayed = 1
                    return
                  }

                  // Update priority tracking for this SVG element
                  var currentHighestPriority = svgElementPriorities[damagedComponent.svgId]
                  if (currentHighestPriority === undefined || damagedComponent.priority > currentHighestPriority) {
                    svgElementPriorities[damagedComponent.svgId] = damagedComponent.priority
                  }

                  if (damagedComponent.priority === 1) {
                    permanentDamage = 1
                    // Only increment permanentDamagedParts for actual permanent damage
                    if (!damagedComponent.tempDamage) {
                      permanentDamagedParts += 1
                    }
                    $timeout.cancel(damageTimeout)
                    // Check if another component on the same SVG has already claimed this message. If so, only update the fill color.
                    // Do NOT call setDamage because its .attr({class:''}) call would strip the flashAnim class that the other component just set on the same element.
                    var messageAlreadyShown = false
                    if (damagedComponent.damageText !== undefined) {
                      var preResolvedText = getVehicleDamageText(damagedComponent.damageText)
                      for (var pt in componentDamageMap) {
                        for (var pc in componentDamageMap[pt]) {
                          var po = componentDamageMap[pt][pc]
                          if (po !== damagedComponent &&
                              po.svgId === damagedComponent.svgId &&
                              po.damageDisplayed === 1 &&
                              getVehicleDamageText(po.damageText) === preResolvedText) {
                            messageAlreadyShown = true
                          }
                        }
                      }
                    }
                    if (messageAlreadyShown) {
                      hu(damagedComponent.svgId, svg).css({ fill: redColor })
                    } else {
                      setDamage(damagedComponent, redColor, 'flashAnim')
                    }
                    newVisualDamage = true // Track that new visual damage occurred
                  } else if (damagedComponent.priority === 0) {
                    permanentDamage = 1
                    if (!damagedComponent.tempDamage) {
                      permanentDamagedParts += 1
                    }
                    $timeout.cancel(damageTimeout)
                    var flashColor = (svgElementPriorities[damagedComponent.svgId] === 1) ? redColor : orangeColor
                    setDamage(damagedComponent, flashColor, 'flashAnim')
                    newVisualDamage = true // Track that new visual damage occurred
                  } else if (damagedComponent.priority === 2) {
                    var damageAmount = Math.round(data[type][component] * 1000)
                    var bodyColor = `rgba(${150+damageAmount}, ${150-damageAmount}, 0, 0.6)`
                    setDamage(damagedComponent, bodyColor, '')
                    newVisualDamage = true // Track that new visual damage occurred (includes body damage)
                  }
                  hasDamage = 1

                  // Don't add temp damage messages to queue if the SVG element has permanent damage
                  if (damagedComponent.damageText !== undefined && damagedComponent.damageDisplayed === 0) {
                    if (!damagedComponent.tempDamage || !hasPermanentDamage(damagedComponent.svgId)) {
                      var resolvedText = getVehicleDamageText(damagedComponent.damageText)
                      // Don't push if another component on the same SVG element already displayed the same message
                      var alreadyShown = false
                      for (var t in componentDamageMap) {
                        for (var c in componentDamageMap[t]) {
                          var other = componentDamageMap[t][c]
                          if (other !== damagedComponent &&
                              other.svgId === damagedComponent.svgId &&
                              other.damageDisplayed === 1 &&
                              getVehicleDamageText(other.damageText) === resolvedText) {
                            alreadyShown = true
                          }
                        }
                      }
                      if (!alreadyShown) {
                        var queueEntry = angular.extend({}, damagedComponent, { damageText: resolvedText })
                        damageQueue.push(queueEntry)
                      }
                    }
                    damagedComponent.damageDisplayed = 1
                  }
                }
              } else if (damagedComponent.tempDamage) {
                if (data[type][component] === true || data[type][component] > 0) {
                  // For temp damage that's already displayed, don't do anything
                  // This prevents re-flashing when other damage updates occur or when permanent damage exists
                  // The visual is already set from when it first appeared, no need to update it again
                }
                else if (data[type][component] === false || data[type][component] === 0) {
                  damagedComponent.damageDisplayed = 0
                  // Note: We don't decrement permanentDamagedParts here because temporary damage
                  // never increments it in the first place (see checkDamage logic above)

                  // Recalculate priority for this SVG element
                  delete svgElementPriorities[damagedComponent.svgId]
                  // Check all other components that use this SVG element
                  // Include both permanent AND still-active temp damage so double temp damage
                  // doesn't clear the icon when only one of the two clears
                  for (var checkType in componentDamageMap) {
                    for (var checkComponent in componentDamageMap[checkType]) {
                      var checkDamageComponent = componentDamageMap[checkType][checkComponent]
                      if (checkDamageComponent.svgId === damagedComponent.svgId &&
                          checkDamageComponent.damageDisplayed === 1) {
                        var currentPriority = svgElementPriorities[damagedComponent.svgId]
                        if (currentPriority === undefined || checkDamageComponent.priority > currentPriority) {
                          svgElementPriorities[damagedComponent.svgId] = checkDamageComponent.priority
                        }
                      }
                    }
                  }

                  // Update the SVG element based on remaining damage
                  updateSvgElement(damagedComponent.svgId)

                  if (permanentDamagedParts === 0 && !hasActiveTempDamage()) {
                    if (damageTimeout) {
                      $timeout.cancel(damageTimeout)
                    }
                    damageTimeout = $timeout(function() {
                      svgCarGroup.animate({opacity: 0}, 200)
                    }, 2500)
                  }
                }
              }
            }
          }

          // Event listeners
          function listenOnce(evt, fn) { svgListeners.push($scope.$on(evt, fn)) }

          listenOnce('DamageData', function(ev, data) {
            // Deep merge into lastDamageData so partial updates from extensions
            // don't wipe out damage data from other sources
            if (!lastDamageData) {
              lastDamageData = {}
            }
            for (var category in data) {
              if (!lastDamageData[category]) lastDamageData[category] = {}
              for (var key in data[category]) {
                lastDamageData[category][key] = data[category][key]
              }
            }

            if (currentVehicleModel) {
              try {
                sessionStorage.setItem('improvedDmg_' + currentVehicleModel, JSON.stringify(lastDamageData))
              } catch(e) {}
            }

            // Reset flag at start of update cycle
            newVisualDamage = false

            for (var key in data) {
              for (var val in data[key]) {
                checkDamage(key, val, data)
              }
            }

            // Update which icons are clickable based on current damage
            updateClickableElements()

            // Show app if there's new damage text to display OR new visual damage occurred
            if (appDisplayed === 0 && (damageQueue.length > 0 || newVisualDamage)) {
              appDisplayed = 1
              showApp(damageQueue)
            }
          })

          listenOnce('DamageMessage', function(ev, data) {
            damageQueue.push(data)
            showApp()
          })

          listenOnce('VehicleReset', function() {
            if (currentVehicleModel) {
              try { sessionStorage.removeItem('improvedDmg_' + currentVehicleModel) } catch(e) {}
            }
            lastDamageData = null
            fetchCurrentVehicleModel()
            reset()
          })

          listenOnce('VehicleChange', function() {
            isInitialLoad = false
            lastDamageData = null
            currentHasDogTeeth = false
            currentHasHeavyDuallyWheels = false
            configChangedSinceReset = false
            reset()
            // Fire immediately so an already-spawned vehicle's part info resolves before the
            // game's own DamageData event arrives, avoiding a visible flash of the wrong
            // Inner/Outer text. The delayed call stays as a safety net for the case where a
            // part was *just* installed and the on-disk pc file genuinely needs time to catch
            // up - the VehiclePartsInfo handler's refreshDamage() call corrects that case too.
            fetchCurrentVehicleModel()
            $timeout(fetchCurrentVehicleModel, 500)
          })

          // request skeleton on TAB
          listenOnce('VehicleFocusChanged', function(evt, data) {
            if (data.mode === true) {
              isInitialLoad = false
              lastDamageData = null
              currentHasDogTeeth = false
              currentHasHeavyDuallyWheels = false
              configChangedSinceReset = false
              reset()
              fetchCurrentVehicleModel()
              $timeout(fetchCurrentVehicleModel, 500)
            }
          })

          // Receives gearbox type info pushed by the ui_vehiclePartsInfo GE Lua extension
          // Fires on initial load (via fetchCurrentVehicleModel) and automatically on every vehicle spawn and focus change via the extension's hooks
          listenOnce('VehiclePartsInfo', function(ev, data) {
            if (data) {
              var previousVehicleModel = currentVehicleModel
              currentVehicleModel = data.model || null
              var modelChanged = previousVehicleModel !== currentVehicleModel
              currentVehId = (typeof data.vehId !== 'undefined') ? data.vehId : null

              var partsInfoChanged = false
              if (!configChangedSinceReset) {
                var newHasDogTeeth = !!data.hasDogTeeth
                var newHasHeavyDuallyWheels = !!data.hasHeavyDuallyWheels

                // Some vehicles/mods don't reliably re-surface their heavy dually part through the pc-file/vehicle-details path this Lua read comes from on every switch,
                // even though the live part tree (VehicleConfigChange) saw it correctly when the part was actually installed.
                // Once confirmed for this specific vehicle instance, trust that over an occasional flaky false reading - keyed by vehId+model so two differently configured vehicles of the same model don't affect each other,
                // AND so a vehId reused by a different model after a despawn can't inherit a stale confirmation from whatever vehicle held that vehId before it.
                var cacheKey = heavyDuallyCacheKey(currentVehId, currentVehicleModel)
                if (currentVehId !== null && heavyDuallyConfirmedByVehId[cacheKey]) {
                  newHasHeavyDuallyWheels = true
                } else if (newHasHeavyDuallyWheels && currentVehId !== null) {
                  heavyDuallyConfirmedByVehId[cacheKey] = true
                  persistHeavyDuallyCache()
                }

                if (newHasDogTeeth !== currentHasDogTeeth || newHasHeavyDuallyWheels !== currentHasHeavyDuallyWheels) {
                  partsInfoChanged = true
                }
                currentHasDogTeeth = newHasDogTeeth
                currentHasHeavyDuallyWheels = newHasHeavyDuallyWheels
              }

              var restoredFromStorage = false
              // Only restore sessionStorage on a fresh F5 load, never on vehicle switch.
              // isInitialLoad is set false by VehicleFocusChanged/VehicleChange so
              // this block is skipped entirely during tab switching.
              if (isInitialLoad && lastDamageData === null && currentVehicleModel) {
                isInitialLoad = false
                try {
                  var stored = sessionStorage.getItem('improvedDmg_' + currentVehicleModel)
                  if (stored) { lastDamageData = JSON.parse(stored); restoredFromStorage = true }
                } catch(e) {}
              }

              // Only re-resolve already-known damage when something that actually affects its wording changed (the model itself, or the dog-teeth/dually flags)
              // or we just restored from storage and need an initial render. refreshDamage() restarts the message display timer every time it runs,
              // so calling it unconditionally on every VehiclePartsInfo event would make messages appear to linger longer than textDisplayTime even when nothing changed.
              if (modelChanged || partsInfoChanged || restoredFromStorage) {
                refreshDamage()
              }
            }
          })

          // Fired by core_partmgmt via guihooks.triggerRawJS after every parts change
          listenOnce('VehicleConfigChange', function(evt, data) {
            if (!data || !data.chosenPartsTree) return
            // Recursively search the parts tree for a node whose id matches slotId
            var findSlotInTree = function(node, slotId) {
              if (!node) return null
              if (node.id === slotId) return node.chosenPartName || null
              var children = node.children || {}
              for (var key in children) {
                var result = findSlotInTree(children[key], slotId)
                if (result !== null) return result
              }
              return null
            }

            var dogTeethEngines = {
              'nine_engine_i4_flathead_134': true,
              'nine_engine_v8_flathead_232': true,
              'nine_engine_v8_ohv_291':      false,
              'nine_engine_v8_ohv_353':      false,
              'nine_engine_v8_ohv_423':      false,
            }

            var enginePart = findSlotInTree(data.chosenPartsTree, 'nine_engine')
            if (enginePart !== null && dogTeethEngines.hasOwnProperty(enginePart)) {
              configChangedSinceReset = true
              currentHasDogTeeth = dogTeethEngines[enginePart]
            }

            // Walks the whole tree once, checking each part's own chosen name rather than requiring the slot id to contain 'hub'.
            // Some mods attach these through a completely different slot system where the slot id never mentions 'hub' at all - the only reliable signal is the chosen part's own name, so we check that directly instead.
            var foundAnyWheelOrHubPart = false
            var foundHeavyDually = false
            var searchForHeavyDually = function(node) {
              if (!node) return
              if (node.chosenPartName) {
                // 'wheel' as a bare substring also matches inside "flywheel" - this caused false positives whenever a vehicle had ANY heavy flywheel installed, completely unrelated to its actual tires/wheels. Excluded explicitly.
                var mentionsFlywheel = /flywheel/i.test(node.chosenPartName)
                var mentionsWheelOrHub = (/wheel/i.test(node.chosenPartName) && !mentionsFlywheel) || /hub/i.test(node.chosenPartName)
                if (mentionsWheelOrHub) {
                  foundAnyWheelOrHubPart = true
                  // Verified against actual JBeam part data: the real heavy 8-lug dually hub's internal name contains 'dual'
                  // but never 'heavy' - "Heavy Duty" only appears in the human-readable information.name, which isn't what we check here.
                  // Native truck dually wheel/tire parts never contain 'dual' in their own part names at all - the only 'dual' in those JBeams is inside a components.
                  // dualyOffsetsX sub-object used for offset math, which is never a chosenPartName. So a bare OR across heavy/dual/8lug is correct here.
                  if (/heavy|dual|8[\s_-]?lug/i.test(node.chosenPartName)) {
                    foundHeavyDually = true
                  }
                }
              }
              var children = node.children || {}
              for (var key in children) { searchForHeavyDually(children[key]) }
            }
            searchForHeavyDually(data.chosenPartsTree)

            // Only update hasHeavyDuallyWheels when at least one wheel/hub-ish part was found
            // in the tree (confirms the tree is complete enough for this vehicle to trust the result)
            // Mark configChangedSinceReset so VehiclePartsInfo won't overwrite with stale pc file data on part changes
            if (foundAnyWheelOrHubPart) {
              configChangedSinceReset = true
              currentHasHeavyDuallyWheels = foundHeavyDually
              // This live tree walk is the most reliable signal we have, so record it into the sticky cache,
              // keyed by vehId+model so two differently configured vehicles of the same model don't affect each other,
              // AND so a vehId reused by a different model after a despawn can't inherit a stale confirmation.
              // Config changes always happen to the currently focused vehicle, so currentVehId/currentVehicleModel (set by the most recent VehiclePartsInfo event) are the right identity here.
              // A confirmed true persists for the rest of the session; a live, explicit removal clears it so this vehicle can be re-evaluated fresh.
              if (currentVehId !== null) {
                var cacheKey = heavyDuallyCacheKey(currentVehId, currentVehicleModel)
                if (foundHeavyDually) {
                  heavyDuallyConfirmedByVehId[cacheKey] = true
                } else {
                  delete heavyDuallyConfirmedByVehId[cacheKey]
                }
                persistHeavyDuallyCache()
              }
            }
          })

          listenOnce('$destroy', function () {
            // Remove all click handlers from damaged elements
            for (var svgId in clickableElements) {
              removeClickFromElement(svgId)
            }
            StreamsManager.remove(streamsList)
          })
        }
      }
    }
  }])

  // Damage text translations
  // Each language is a key containing { "English damageText": "Translated text" }.
  // The English text must exactly match the damageText values used above in componentDamageMap (case-sensitive).
  // If a language or a specific message isn't listed, this falls back to English.
  // For region varients, use the full locale as the key instead, lowercase with an underscore - e.g. 'pt_pt' and 'pt_br'.
  // Those are checked first, before a plain 'pt' fallback, so each region only needs to list what differs from the other.
  var damageTranslation = {
    // French (France)
    fr: {
      "Coolant Overheating": "Liquide de refroidissement en surchauffe",
      "Turbocharger Overheating": "Surchauffe du turbocompresseur",
      "Clutch Overheating": "Surchauffe de l'embrayage",
      "Oil Overheating": "Huile en surchauffe",
      "Oil Starvation Damage": "Dégâts du manque d'huile",
      "Low Oil Pressure": "Pression basse d'huile",
      "High Oil Pressure": "Pression élevée d'huile",
      "Piston Rings Damaged": "Segments de piston endommagés",
      "Rod Bearings Damaged": "Roulements de bielle endommagés",
      "Head Gasket Blown": "Joint de culasse soufflé",
      "Engine is Hydrolocking": "Le moteur est en train de se bloquer",
      "Engine Torque Reduced": "Couple moteur réduit",
      "Mild Over Rev Damage": "Dégâts de surrégime légers",
      "Catastrophic Over Rev Damage": "Dégâts de surrégime catastrophiques",
      "Over Rev Danger": "Danger de surrégime",
      "Over Torque Danger": "Danger de surcouple",
      "Catastrophic Over Torque Damage": "Dégâts de surcouple catastrophiques",
      "Engine is Hydrolocked": "Le moteur est bloqué",
      "Engine Disabled": "Moteur désactivé",
      "Engine Block Melted": "Bloc-moteur fondu",
      "Cylinder Walls Melted": "Parois de cylindre fondues",
      "Engine Locked Up": "Moteur bloqué",
      "Radiator Leaking": "Fuite du radiateur",
      "Oil Radiator Leaking": "Fuite du radiateur d'huile",
      "Oil Pan Leaking": "Fuite du carter d'huile",
      "Induction System Damage": "Dégâts du système d'induction",
      "Engine Impact Damage": "Dégâts d'impact sur le moteur",
      "Engine Block Damage": "Dégâts sur le bloc-moteur",
      "Cylinder Wall Damage": "Dégâts sur la paroi de cylindre",
      "Synchronizer Damage": "Dégâts du synchronisateur",
      "Gear Dog Damage": "Dégâts du chien de l'engrenage",

      "Front Left Axle Broken": "Essieu avant gauche détruit",
      "Front Right Axle Broken": "Essieu avant droit détruit",
      "Rear Left Axle Broken": "Essieu arrière gauche détruit",
      "Rear Right Axle Broken": "Essieu arrière droit détruit",
      "Rear Left Hub Broken": "Moyeu arrière gauche détruit",
      "Rear Right Hub Broken": "Moyeu arrière droit détruit",
      "Front Left Spindle Broken": "Fusée avant gauche détruite",
      "Front Right Spindle Broken": "Fusée avant droit détruite",
      "Rear Left Spindle Broken": "Fusée arrière gauche détruite",
      "Rear Right Spindle Broken": "Fusée arrière droit détruite",
      "Rear Driveshaft Broken": "Arbre de transmission arrière cassé",
      "Middle Driveshaft Broken": "Arbre de transmission central cassé",
      "Front Driveshaft Broken": "Arbre de transmission avant cassé",
      "Front Front Driveshaft Broken": "Arbre de transmission avant avant cassé",
      "Front Rear Driveshaft Broken": "Arbre de transmission avant arrière cassé",
      "Left Driveshaft Broken": "Arbre de transmission gauche cassé",
      "Right Driveshaft Broken": "Arbre de transmission droit cassé",
      "Propshaft Broken": "Arbre de transmission cassé",
      "Intershaft Broken": "Arbre intermédiaire cassé",
      "Rear Intershaft Broken": "Arbre intermédiaire arrière cassé",
      "Transfer Case Driveshaft Broken": "Arbre de transmission de la boîte de transfert cassé",
      "Main Engine Broken": "Moteur principal cassé",
      "Front Motor Broken": "Moteur électrique avant cassé",
      "Rear Motor Broken": "Moteur électrique arrière cassé",

      "Fuel Tank Damaged": "Réservoir de carburant endommagé",
      "Left Fuel Tank Damaged": "Réservoir de carburant gauche endommagé",
      "Right Fuel Tank Damaged": "Réservoir de carburant droit endommagé",
      "Fuel Cell Damaged": "Pile à carburant endommagée",
      "Pressure Tank Damaged": "Réservoir de pression endommagé",

      "Front Left Tire Flat": "Pneu avant gauche dégonflé",
      "Front Right Tire Flat": "Pneu avant droit dégonflé",
      "Rear Left Tire Flat": "Pneu arrière gauche dégonflé",
      "Rear Right Tire Flat": "Pneu arrière droit dégonflé",
      "Rear Left Outer Tire Flat": "Pneu arrière gauche extérieur dégonflé",
      "Rear Left Inner Tire Flat": "Pneu arrière gauche intérieur dégonflé",
      "Rear Right Outer Tire Flat": "Pneu arrière droit extérieur dégonflé",
      "Rear Right Inner Tire Flat": "Pneu arrière droit intérieur dégonflé",
      "Front Left Tire Punctured": "Pneu avant gauche crevé",
      "Front Right Tire Punctured": "Pneu avant droit crevé",
      "Rear Left Tire Punctured": "Pneu arrière gauche crevé",
      "Rear Right Tire Punctured": "Pneu arrière droit crevé",
      "Rear Left Outer Tire Punctured": "Pneu arrière gauche extérieur crevé",
      "Rear Left Inner Tire Punctured": "Pneu arrière gauche intérieur crevé",
      "Rear Right Outer Tire Punctured": "Pneu arrière droit extérieur crevé",
      "Rear Right Inner Tire Punctured": "Pneu arrière droit intérieur crevé",
      "Front Left Brake Damaged": "Frein avant gauche détruit",
      "Front Right Brake Damaged": "Frein avant droit détruit",
      "Rear Left Brake Damaged": "Frein arrière gauche détruit",
      "Rear Right Brake Damaged": "Frein arrière droit détruit",
      "Front Left Brake Fading": "Évanouissement du frein avant gauche",
      "Front Right Brake Fading": "Évanouissement du frein avant droit",
      "Rear Left Brake Fading": "Évanouissement du frein arrière gauche",
      "Rear Right Brake Fading": "Évanouissement du frein arrière droit",
      "Front Left Wheel Detached": "Roue avant gauche détachée",
      "Front Right Wheel Detached": "Roue avant droite détachée",
      "Rear Left Wheel Detached": "Roue arrière gauche détachée",
      "Rear Right Wheel Detached": "Roue arrière droit détachée"
    },
    // German (Germany)
    de: {
      "Coolant Overheating": "Überhitzung des Kühlmittels",
      "Turbocharger Overheating": "Überhitzung des Turboladers",
      "Clutch Overheating": "Überhitzung der Kupplung",
      "Oil Overheating": "Ölüberhitzung",
      "Oil Starvation Damage": "Ölmangelschäden",
      "Low Oil Pressure": "Niedriger Öldruck",
      "High Oil Pressure": "Hoher Öldruck",
      "Piston Rings Damaged": "Kolbenringe beschädigt",
      "Rod Bearings Damaged": "Pleuellager beschädigt",
      "Head Gasket Blown": "Kopfdichtung geblasen",
      "Engine is Hydrolocking": "Wasserschlag im Motor droht",
      "Engine Torque Reduced": "Motordrehmoment verringert",
      "Mild Over Rev Damage": "Leichter Schaden durch Überdrehung",
      "Catastrophic Over Rev Damage": "Katastrophaler Schaden durch Überdrehung",
      "Over Rev Danger": "Überdrehungsgefahr",
      "Over Torque Danger": "Überdrehmomentgefahr",
      "Catastrophic Over Torque Damage": "Katastrophaler Schaden durch Überdrehmoment",
      "Engine is Hydrolocked": "Wasserschlag im Motor",
      "Engine Disabled": "Motor deaktiviert",
      "Engine Block Melted": "Motorblock geschmolzen",
      "Cylinder Walls Melted": "Zylinderwände geschmolzen",
      "Engine Locked Up": "Motor blockiert",
      "Radiator Leaking": "Kühlerleck",
      "Oil Radiator Leaking": "Ölkühlerleck",
      "Oil Pan Leaking": "Ölwannenleck",
      "Induction System Damage": "Schaden am Induktionssystem",
      "Engine Impact Damage": "Aufprallschaden am Motor",
      "Engine Block Damage": "Motorblockschaden",
      "Cylinder Wall Damage": "Zylinderwandschaden",
      "Synchronizer Damage": "Synchronisatorschaden",
      "Gear Dog Damage": "Schaltklauenschaden",

      "Front Left Axle Broken": "Linke Vorderachse defekt",
      "Front Right Axle Broken": "Rechte Vorderachse defekt",
      "Rear Left Axle Broken": "Linke Hinterachse defekt",
      "Rear Right Axle Broken": "Rechte Hinterachse defekt",
      "Rear Left Hub Broken": "Linke Hinterenabe defekt",
      "Rear Right Hub Broken": "Rechte Hinterradnabe defekt",
      "Front Left Spindle Broken": "Linker Vorderachszapfen defekt",
      "Front Right Spindle Broken": "Rechter Vorderachszapfen defekt",
      "Rear Left Spindle Broken": "Linker Hinterachszapfen defekt",
      "Rear Right Spindle Broken": "Rechter Hinterachszapfen defekt",
      "Rear Driveshaft Broken": "Hintere Antriebswelle defekt",
      "Middle Driveshaft Broken": "Mittlere Antriebswelle defekt",
      "Front Driveshaft Broken": "Vordere Antriebswelle defekt",
      "Front Front Driveshaft Broken": "Vorderste Antriebswelle defekt",
      "Front Rear Driveshaft Broken": "Hintere vordere Antriebswelle defekt",
      "Left Driveshaft Broken": "Linke Antriebswelle defekt",
      "Right Driveshaft Broken": "Rechte Antriebswelle defekt",
      "Propshaft Broken": "Kardanwelle defekt",
      "Intershaft Broken": "Zwischenwelle defekt",
      "Rear Intershaft Broken": "Hintere Zwischenwelle defekt",
      "Transfer Case Driveshaft Broken": "Verteilergetriebe-Antriebswelle defekt",
      "Main Engine Broken": "Hauptmotor defekt",
      "Front Motor Broken": "Vordermotor defekt",
      "Rear Motor Broken": "Heckmotor defekt",

      "Fuel Tank Damaged": "Tank beschädigt",
      "Left Fuel Tank Damaged": "Linke Tank beschädigt",
      "Right Fuel Tank Damaged": "Rechte Tank beschädigt",
      "Fuel Cell Damaged": "Zelle beschädigt",
      "Pressure Tank Damaged": "Drucktank beschädigt",

      "Front Left Tire Flat": "Linker Vorderreifen platt",
      "Front Right Tire Flat": "Rechter Vorderreifen platt",
      "Rear Left Tire Flat": "Linker Hinterreifen platt",
      "Rear Right Tire Flat": "Rechter Hinterreifen platt",
      "Rear Left Outer Tire Flat": "Äußerer linker Hinterreifen platt",
      "Rear Left Inner Tire Flat": "Innerer linker Hinterreifen platt",
      "Rear Right Outer Tire Flat": "Äußerer rechter Hinterreifen platt",
      "Rear Right Inner Tire Flat": "Innerer rechter Hinterreifen platt",
      "Front Left Tire Punctured": "Linker Vorderreifen durchstochen",
      "Front Right Tire Punctured": "Rechter Vorderreifen durchstochen",
      "Rear Left Tire Punctured": "Linker Hinterreifen durchstochen",
      "Rear Right Tire Punctured": "Rechter Hinterreifen durchstochen",
      "Rear Left Outer Tire Punctured": "Äußerer linker Hinterreifen durchstochen",
      "Rear Left Inner Tire Punctured": "Innerer linker Hinterreifen durchstochen",
      "Rear Right Outer Tire Punctured": "Äußerer rechter Hinterreifen durchstochen",
      "Rear Right Inner Tire Punctured": "Innerer rechter Hinterreifen durchstochen",
      "Front Left Brake Damaged": "Linke Vorderbremse beschädigt",
      "Front Right Brake Damaged": "Rechte Vorderbremse beschädigt",
      "Rear Left Brake Damaged": "Linke Hinterbremse beschädigt",
      "Rear Right Brake Damaged": "Rechte Hinterbremse beschädigt",
      "Front Left Brake Fading": "Bremsschwund vorne links",
      "Front Right Brake Fading": "Bremsschwund vorne rechts",
      "Rear Left Brake Fading": "Bremsschwund hinten links",
      "Rear Right Brake Fading": "Bremsschwund hinten rechts",
      "Front Left Wheel Detached": "Linkes Vorderrad abgelöst",
      "Front Right Wheel Detached": "Rechtes Vorderrad abgelöst",
      "Rear Left Wheel Detached": "Linkes Hinterrad abgelöst",
      "Rear Right Wheel Detached": "Rechtes Hinterrad abgelöst"
    },
    // Spanish (LATAM)
    es_419: {
      "Coolant Overheating": "Sobrecalentamiento del refrigerante",
      "Turbocharger Overheating": "Sobrecalentamiento del turbo",
      "Clutch Overheating": "Sobrecalentamiento del embrague",
      "Oil Overheating": "Sobrecalentamiento de aceite",
      "Oil Starvation Damage": "Daño por deprivación de aceite",
      "Low Oil Pressure": "Baja presión de aceite",
      "High Oil Pressure": "Alta presión de aceite",
      "Piston Rings Damaged": "Anillos de pistón dañados",
      "Rod Bearings Damaged": "Bielas dañadas",
      "Head Gasket Blown": "Junta de culata soplada",
      "Engine is Hydrolocking": "El motor se está ahogando",
      "Engine Torque Reduced": "Torque de motor reducido",
      "Mild Over Rev Damage": "Daño moderado de sobrerevoluciones",
      "Catastrophic Over Rev Damage": "Daño catastrófico de sobrerevoluciones",
      "Over Rev Danger": "Peligro de sobrerevolución",
      "Over Torque Danger": "Peligro de sobretorque",
      "Catastrophic Over Torque Damage": "Daño catastrófico de sobretorque",
      "Engine is Hydrolocked": "El motor está ahogado",
      "Engine Disabled": "Motor desactivado",
      "Engine Block Melted": "Se derritió el bloque del motor",
      "Cylinder Walls Melted": "Se derritieron las paredes de los cilindros",
      "Engine Locked Up": "Motor bloqueado",
      "Radiator Leaking": "Fuga en el radiador",
      "Oil Radiator Leaking": "Fuga en el radiador de aceite",
      "Oil Pan Leaking": "Fuga en el cárter",
      "Induction System Damage": "Sistema de inducción dañado",
      "Engine Impact Damage": "Daño de impacto en el motor",
      "Engine Block Damage": "Daño en el bloque del motor",
      "Cylinder Wall Damage": "Daño en las paredes de los cilindros",
      "Synchronizer Damage": "Daño de sincronizador",
      "Gear Dog Damage": "Daño de dientes de acople",

      "Front Left Axle Broken": "Torno frontal izquierdo roto",
      "Front Right Axle Broken": "Torno frontal derecho roto",
      "Rear Left Axle Broken": "Torno trasero izquierdo roto",
      "Rear Right Axle Broken": "Torno trasero derecho roto",
      "Rear Left Hub Broken": "Cubo trasero izquierdo roto",
      "Rear Right Hub Broken": "Cubo trasero derecho roto",
      "Front Left Spindle Broken": "Mangueta frontal izquierda rota",
      "Front Right Spindle Broken": "Mangueta frontal derecha rota",
      "Rear Left Spindle Broken": "Mangueta trasera izquierda rota",
      "Rear Right Spindle Broken": "Mangueta trasera derecha rota",
      "Rear Driveshaft Broken": "Transmisión trasera rota",
      "Middle Driveshaft Broken": "Transmisión central rota",
      "Front Driveshaft Broken": "Transmisión frontal rota",
      "Front Front Driveshaft Broken": "Transmisión frontal delantera rota",
      "Front Rear Driveshaft Broken": "Transmisión trasera delantera rota",
      "Left Driveshaft Broken": "Transmisión izquierda rota",
      "Right Driveshaft Broken": "Transmisión derecha rota",
      "Propshaft Broken": "Eje de transmisión roto",
      "Intershaft Broken": "Eje intermedio roto",
      "Rear Intershaft Broken": "Eje intermedio trasero roto",
      "Transfer Case Driveshaft Broken": "Transmisión de caja de transferencia rota",
      "Main Engine Broken": "Motor principal descompuesto",
      "Front Motor Broken": "Motor frontal descompuesto",
      "Rear Motor Broken": "Motor trasero descompuesto",

      "Fuel Tank Damaged": "Tanque de combustible dañado",
      "Left Fuel Tank Damaged": "Tanque de combustible izquierdo dañado",
      "Right Fuel Tank Damaged": "Tanque de combustible derecho dañado",
      "Fuel Cell Damaged": "Célula de combustible dañada",
      "Pressure Tank Damaged": "Tanque de presión dañado",

      "Front Left Tire Flat": "Llanta delantera izquierda desinflada",
      "Front Right Tire Flat": "Llanta delantera derecha desinflada",
      "Rear Left Tire Flat": "Llanta trasera izquierda desinflada",
      "Rear Right Tire Flat": "Llanta trasera derecha desinflada",
      "Rear Left Outer Tire Flat": "Llanta trasera izquierda exterior desinflada",
      "Rear Left Inner Tire Flat": "Llanta trasera izquierda interior desinflada",
      "Rear Right Outer Tire Flat": "Llanta trasera derecha exterior desinflada",
      "Rear Right Inner Tire Flat": "Llanta trasera derecha interior desinflada",
      "Front Left Tire Punctured": "Llanta delantera izquierda pinchada",
      "Front Right Tire Punctured": "Llanta delantera derecha pinchada",
      "Rear Left Tire Punctured": "Llanta trasera izquierda pinchada",
      "Rear Right Tire Punctured": "Llanta trasera derecha pinchada",
      "Rear Left Outer Tire Punctured": "Llanta trasera izquierda exterior pinchada",
      "Rear Left Inner Tire Punctured": "Llanta trasera izquierda interior pinchada",
      "Rear Right Outer Tire Punctured": "Llanta trasera derecha exterior pinchada",
      "Rear Right Inner Tire Punctured": "Llanta trasera derecha interior pinchada",
      "Front Left Brake Damaged": "Freno delantero izquierdo dañado",
      "Front Right Brake Damaged": "Freno delantero derecho dañado",
      "Rear Left Brake Damaged": "Freno trasero izquierdo dañado",
      "Rear Right Brake Damaged": "Freno trasero derecho dañado",
      "Front Left Brake Fading": "Freno delantero izquierdo desgastado",
      "Front Right Brake Fading": "Freno delantero derecho desgastado",
      "Rear Left Brake Fading": "Freno trasero izquierdo desgastado",
      "Rear Right Brake Fading": "Freno trasero derecho desgastado",
      "Front Left Wheel Detached": "Rueda frontal izquierda desprendida",
      "Front Right Wheel Detached": "Rueda frontal derecha desprendida",
      "Rear Left Wheel Detached": "Rueda trasera izquierda desprendida",
      "Rear Right Wheel Detached": "Rueda trasera derecha desprendida"
    },
    // Spanish (Spain)
    es_es: {
      "Coolant Overheating": "Refrigerante sobrecalentado",
      "Turbocharger Overheating": "Sobrecalentamiento del turbo",
      "Clutch Overheating": "Sobrecalentamiento del embrague",
      "Oil Overheating": "Sobrecalentamiento de aceite",
      "Oil Starvation Damage": "Daño por falta de aceite",
      "Low Oil Pressure": "Presión de aceite baja",
      "High Oil Pressure": "Presión de aceite alta",
      "Piston Rings Damaged": "Aros de pistón dañados",
      "Rod Bearings Damaged": "Cojinetes dañados",
      "Head Gasket Blown": "Junta de culata quemada",
      "Engine is Hydrolocking": "El motor se está hidrobloqueando",
      "Engine Torque Reduced": "Par motor del motor reducido",
      "Mild Over Rev Damage": "Daño leve por exceso de revoluciones",
      "Catastrophic Over Rev Damage": "Daño fatal por exceso de revoluciones",
      "Over Rev Danger": "Peligro por exceso de revoluciones",
      "Over Torque Danger": "Peligro por exceso de par motor",
      "Catastrophic Over Torque Damage": "Daño fatal por exceso de par motor",
      "Engine is Hydrolocked": "El motor está hidrobloqueado",
      "Engine Disabled": "Motor deshabilitado",
      "Engine Block Melted": "Bloque motor derretido",
      "Cylinder Walls Melted": "Paredes de cilindros derretidas",
      "Engine Locked Up": "Motor ahogado",
      "Radiator Leaking": "Fuga del radiador",
      "Oil Radiator Leaking": "Fuga del radiador de aceite",
      "Oil Pan Leaking": "Fuga del recogedor de aceite",
      "Induction System Damage": "Daño del sistema de inducción",
      "Engine Impact Damage": "Daño por impacto al motor",
      "Engine Block Damage": "Daño en el bloque motor",
      "Cylinder Wall Damage": "Daño en las paredes de cilindros",
      "Synchronizer Damage": "Daño en el sincronizador",
      "Gear Dog Damage": "Daño en el crabot",

      "Front Left Axle Broken": "Eje frontal izquierdo roto",
      "Front Right Axle Broken": "Eje frontal derecho roto",
      "Rear Left Axle Broken": "Eje trasero izquierdo roto",
      "Rear Right Axle Broken": "Eje trasero derecho roto",
      "Rear Left Hub Broken": "Buje trasero izquierdo roto",
      "Rear Right Hub Broken": "Buje trasero derecho roto",
      "Front Left Spindle Broken": "Mangueta frontal izquierda rota",
      "Front Right Spindle Broken": "Mangueta frontal derecha rota",
      "Rear Left Spindle Broken": "Mangueta trasera izquierda rota",
      "Rear Right Spindle Broken": "Mangueta trasera derecha rota",
      "Rear Driveshaft Broken": "Eje de transmisión trasero roto",
      "Middle Driveshaft Broken": "Eje de transmisión central roto",
      "Front Driveshaft Broken": "Eje de transmisión frontal roto",
      "Front Front Driveshaft Broken": "Eje de transmisión frontal anterior roto",
      "Front Rear Driveshaft Broken": "Eje de transmisión frontal trasero roto",
      "Left Driveshaft Broken": "Eje de transmisión izquierdo roto",
      "Right Driveshaft Broken": "Eje de transmisión derecho roto",
      "Propshaft Broken": "Eje de transmisión roto",
      "Intershaft Broken": "Eje de transmisión intermedio roto",
      "Rear Intershaft Broken": "Eje de transmisión intermedio trasero roto",
      "Transfer Case Driveshaft Broken": "Eje de transmisión del tránsfer roto",
      "Main Engine Broken": "Motor principal roto",
      "Front Motor Broken": "Motor frontal roto",
      "Rear Motor Broken": "Motor trasero roto",

      "Fuel Tank Damaged": "Depósito de gasolina dañado",
      "Left Fuel Tank Damaged": "Depósito de gasolina izquierdo dañado",
      "Right Fuel Tank Damaged": "Depósito de gasolina derecho dañado",
      "Fuel Cell Damaged": "Célula de combustible dañada",
      "Pressure Tank Damaged": "Depósito de presión dañado",

      "Front Left Tire Flat": "Desinflado del neumático frontal izquierdo",
      "Front Right Tire Flat": "Desinflado del neumático frontal derecho",
      "Rear Left Tire Flat": "Desinflado del neumático trasero izquierdo",
      "Rear Right Tire Flat": "Desinflado del neumático trasero derecho",
      "Rear Left Outer Tire Flat": "Desinflado del neumático trasero izquierdo exterior",
      "Rear Left Inner Tire Flat": "Desinflado del neumático trasero izquierdo interior",
      "Rear Right Outer Tire Flat": "Desinflado del neumático trasero derecho exterior",
      "Rear Right Inner Tire Flat": "Desinflado del neumático trasero derecho interior",
      "Front Left Tire Punctured": "Pinchazo del neumático frontal izquierdo",
      "Front Right Tire Punctured": "Pinchazo del neumático frontal derecho",
      "Rear Left Tire Punctured": "Pinchazo del neumático trasero izquierdo",
      "Rear Right Tire Punctured": "Pinchazo del neumático trasero derecho",
      "Rear Left Outer Tire Punctured": "Pinchazo del neumático trasero izquierdo exterior",
      "Rear Left Inner Tire Punctured": "Pinchazo del neumático trasero izquierdo interior",
      "Rear Right Outer Tire Punctured": "Pinchazo del neumático trasero derecho exterior",
      "Rear Right Inner Tire Punctured": "Pinchazo del neumático trasero derecho interior",
      "Front Left Brake Damaged": "Freno frontal izquierdo dañado",
      "Front Right Brake Damaged": "Freno frontal derecho dañado",
      "Rear Left Brake Damaged": "Freno trasero izquierdo dañado",
      "Rear Right Brake Damaged": "Freno trasero derecho dañado",
      "Front Left Brake Fading": "Freno frontal izquierdo deteriorado",
      "Front Right Brake Fading": "Freno frontal derecho deteriorado",
      "Rear Left Brake Fading": "Freno trasero izquierdo deteriorado",
      "Rear Right Brake Fading": "Freno trasero derecho deteriorado",
      "Front Left Wheel Detached": "Rueda frontal izquierda desprendida",
      "Front Right Wheel Detached": "Rueda frontal derecha desprendida",
      "Rear Left Wheel Detached": "Rueda trasera izquierda desprendida",
      "Rear Right Wheel Detached": "Rueda trasera derecha desprendida"
    }

  }

  // Reads the in-game UI language the same way BeamNG itself does, normalized
  // to lowercase with underscores - e.g. 'pt-BR' or 'pt_BR' -> 'pt_br'.
  // Defaults to 'en' if not set.
  function currentUiLocale() {
    var raw = (window.UiUnits && window.UiUnits.userSettings && window.UiUnits.userSettings.uiLanguage) || 'en'
    return String(raw).toLowerCase().replace(/-/g, '_')
  }
})()