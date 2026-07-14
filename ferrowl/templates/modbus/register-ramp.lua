-- Ramp a register up and down between two bounds, one step per sim cycle.
-- The sim restarts with fresh globals on every script edit, so `direction` restarts too.
local REGISTER = "Power"
local MIN = 0
local MAX = 100
local STEP = 5

direction = direction or 1

if not C_Register:Has(REGISTER) then
    C_Log:Warn("register-ramp: no register '" .. REGISTER .. "'")
    return
end

local value = C_Register:Get(REGISTER) + STEP * direction

if value >= MAX then
    value = MAX
    direction = -1
elseif value <= MIN then
    value = MIN
    direction = 1
end

C_Register:Set(REGISTER, value)
