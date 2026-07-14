-- Drive a register with a sine wave derived from the module clock.
local REGISTER = "power"
local AMPLITUDE = 50
local OFFSET = 50
local PERIOD = 60 -- seconds per full wave

if not C_Register:Has(REGISTER) then
    C_Log:Warn("sine-wave: no register '" .. REGISTER .. "'")
    return
end

local t = C_Time:Get()
local value = OFFSET + AMPLITUDE * math.sin(2 * math.pi * t / PERIOD)

C_Register:Set(REGISTER, value)
