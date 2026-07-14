-- Script to apply a given input limit as the current power draw
-- Created By: @TumbleOwlee

-- If condition is false, all target registers are set to 0
local condition = true

-- Register definitions, set nil for unsupported
local set_point = "SetPoint"
local set_point_unit = "A" -- or "W"
local set_point_resolution = 1

-- Actual values
local power_resolution = 3.0 -- for 3 phases
local power = "Power"

local current_resolution = 1
local current_l1 = "Current L1"
local current_l2 = "Current L2"
local current_l3 = "Current L3"

-- Get current limit
local limit_a = C_Register:Get(set_point) * set_point_resolution

if set_point_unit == "W" then
    limit_a = limit_a / 230.0
end

if not condition then
    limit_a = 0
end

-- Set all values
if current_l1 then
    C_Register:Set(current_l1, limit_a * current_resolution)
end

if current_l2 then
    C_Register:Set(current_l2, limit_a * current_resolution)
end

if current_l3 then
    C_Register:Set(current_l3, limit_a * current_resolution)
end

if power then
    C_Register:Set(power, limit_a * 230.0 * power_resolution)
end
