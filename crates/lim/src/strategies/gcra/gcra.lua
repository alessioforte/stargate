--[[
High-Precision GCRA (Generic Cell Rate Algorithm)
using Redis TIME (microseconds precision)

KEYS[1] = key (e.g., "gcra:{user_id}")
ARGV[1] = tau (τ in microseconds)
ARGV[2] = burst (B in microseconds)
ARGV[3] = key_ttl (seconds)

Returns:
{ allowed (1 or 0), retry_after, remaining_burst }
]]

local key   = KEYS[1]
local tau   = tonumber(ARGV[1])
local burst = tonumber(ARGV[2])
local ttl   = tonumber(ARGV[3])

if not tau or tau <= 0 or not burst or burst < 0 or not ttl or ttl <= 0 then
    return redis.error_reply("Invalid arguments")
end

-- Get Redis time (seconds, microseconds)
local time_parts = redis.call("TIME")
local t0         = (tonumber(time_parts[1]) * 1000000) + tonumber(time_parts[2])

if not time_parts or #time_parts < 2 then
    return redis.error_reply("Failed to get Redis TIME")
end

-- Read stored Theoretical Arrival Time (TAT)
local tat = redis.call("GET", key)
tat       = tonumber(tat)

if not tat then
    tat = t0 - tau
end

-- Earliest allowed arrival time
local allowed_at = tat - burst

-- Too early → reject
if t0 < allowed_at then
    local retry_after = allowed_at - t0
    return { 0, retry_after, 0 }
end

-- Accept → advance TAT
local new_tat = math.max(tat, t0) + tau

-- Save TAT and TTL
redis.call("SET", key, tostring(new_tat), "EX", ttl)

local remaining_burst = (new_tat - t0) - tau
if remaining_burst < 0 then remaining_burst = 0 end

return { 1, 0, remaining_burst }
