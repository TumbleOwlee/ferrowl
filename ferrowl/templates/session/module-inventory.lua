-- List every module in the session with its type and role, once per cycle.
for _, name in ipairs(C_Module:List()) do
    local m = C_Module:Get(name)

    print(name .. ": " .. m:Type() .. "/" .. m:Role())
end
