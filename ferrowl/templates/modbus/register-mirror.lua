-- Mirror one register into another every cycle.
-- Adjust the names to registers this device actually has.
local SOURCE = "SetPoint"
local TARGET = "Power"

if C_Register:Has(SOURCE) and C_Register:Has(TARGET) then
    C_Register:Set(TARGET, C_Register:Get(SOURCE))
else
    C_Log:Warn("register-mirror: '" .. SOURCE .. "' or '" .. TARGET .. "' does not exist")
end
