--[[
Fixed Window Counter (Quota Tracker)
using Redis TIME for consistent timing

KEYS[1] = bucket hash key
ARGV[1] = limit (maximum requests per window)
ARGV[2] = window (window size in seconds)
ARGV[3] = cost (number of tokens to consume, typically 1)

Returns:
{ allowed (1 or 0), count (current count after increment), ttl_remaining (seconds until window reset) }

The window lives in hash fields instead of a derived key: a script may only
access declared keys on a Redis Cluster node, and deriving `key:window:start`
here would reach a key outside KEYS. Both the window marker and the counter are
fields of the same key, so one entity tag covers the whole operation.
]]
local key    = KEYS[1]
local limit  = tonumber(ARGV[1])
local window = tonumber(ARGV[2])
local cost   = tonumber(ARGV[3])

-- Validate arguments
if not limit or limit <= 0 then
    return redis.error_reply("Invalid limit argument")
end

if not window or window <= 0 then
    return redis.error_reply("Invalid window argument")
end

if not cost or cost < 0 then
    return redis.error_reply("Invalid cost argument")
end

-- Get Redis time (seconds, microseconds)
local time_parts = redis.call("TIME")
if not time_parts or #time_parts < 2 then
    return redis.error_reply("Failed to get Redis TIME")
end

local now_secs = tonumber(time_parts[1])

-- Calculate window boundaries
local window_start = math.floor(now_secs / window) * window
local window_end = window_start + window
local ttl_remaining = window_end - now_secs

-- Fields are scoped by window size so two named limits that share a key but
-- not a window do not read each other's counters.
local start_field = "w:" .. window
local count_field = "c:" .. window

-- Read the counter only when it belongs to the current window. A denied
-- request must not write: a new key would otherwise stay without a TTL.
local stored_start = redis.call("HGET", key, start_field)
local current = 0
if stored_start == tostring(window_start) then
    current = tonumber(redis.call("HGET", key, count_field)) or 0
end

-- Check if adding cost would exceed limit
if current + cost > limit then
    -- Denied - return current count without writing anything
    return { 0, current, ttl_remaining }
end

-- Allowed - write the current window and counter and keep the hash until just
-- after the window ends. Refreshing the TTL also moves it to the next window.
local new_count = current + cost
redis.call("HSET", key, start_field, window_start, count_field, new_count)
redis.call("EXPIRE", key, ttl_remaining + 1)

return { 1, new_count, ttl_remaining }
