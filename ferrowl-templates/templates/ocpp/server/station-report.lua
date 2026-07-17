-- description: Sum the Power reported by every connected station
-- Report every connected charging station and the Power its connectors report.
local total = 0
local stations = C_OCPP:GetChargingStations()

for _, cs in ipairs(stations) do
    for _, id in ipairs(C_OCPP:GetConnectors(cs)) do
        local con = C_OCPP:Connector(cs, id)

        -- A connector that has not reported yet has no Power field: guard the read.
        local ok, power = pcall(function()
            return con:Get("Power")
        end)

        if ok and power then
            total = total + power
        end
    end
end

print("[" .. #stations .. " stations] Total Power = " .. total)
