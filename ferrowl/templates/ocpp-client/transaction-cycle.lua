-- Charge in a loop on connector 1: start a transaction, run for CHARGE_S, stop, idle for IDLE_S.
-- Globals reset whenever the script is edited, so the cycle restarts from Idle.
local CONNECTOR = 1
local CHARGE_S = 30
local IDLE_S = 10
local ID_TAG = "ABC123"

phase = phase or "idle"
since = since or C_Time:Get()

local con = C_OCPP:Connector(CONNECTOR)
local now = C_Time:Get()

if phase == "idle" and now - since >= IDLE_S then
    con:StartTransaction({ idTag = ID_TAG })
    C_Log:Info("transaction-cycle: started on connector " .. CONNECTOR)
    phase = "charging"
    since = now
elseif phase == "charging" then
    con:MeterValues()

    if now - since >= CHARGE_S then
        con:StopTransaction()
        C_Log:Info("transaction-cycle: stopped on connector " .. CONNECTOR)
        phase = "idle"
        since = now
    end
end
