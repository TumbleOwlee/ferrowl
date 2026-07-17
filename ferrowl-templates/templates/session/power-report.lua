-- description: Sum Power across every modbus server and OCPP connector
-- Demo: read `Power` from every modbus server and OCPP connector via C_Module.
local total_power = 0
local num_modbus = 0
local num_ocpp = 0

-- Go over all modules
for _, name in ipairs(C_Module:List()) do
    -- Get module context
    local m = C_Module:Get(name)
    local ty = m:Type()
    local r = m:Role()

    if ty == "modbus" and r == "server" then
        -- Get power of modbus charger
        local reg = m:Register()

        -- Check if register exists before access
        if reg:Has("Power") then
            total_power = total_power + reg:Get("Power")
        end

        num_modbus = num_modbus + 1
    elseif ty == "ocpp" and r == "client" then
        -- Get power of ocpp charger
        local o = m:OCPP()

        -- Get all available connector ids
        for _, i in ipairs(o:GetConnectors()) do
            -- Get connector context
            local con = o:Connector(i)
            total_power = total_power + con:Get("Power")

            num_ocpp = num_ocpp + 1
        end
    end
end

-- Print to log
print("[M" .. num_modbus .. "|O" .. num_ocpp .. "] Total Power = " .. total_power)
