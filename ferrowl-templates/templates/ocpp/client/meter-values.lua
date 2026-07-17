-- description: Set each connector's Power and send MeterValues
-- Raise the meter reading of every connector and send MeterValues once per cycle.
local POWER = 11000 -- W, written to each connector before reporting

for _, id in ipairs(C_OCPP:GetConnectors()) do
    local con = C_OCPP:Connector(id)

    con:Set("Power", POWER)
    con:MeterValues()
end
