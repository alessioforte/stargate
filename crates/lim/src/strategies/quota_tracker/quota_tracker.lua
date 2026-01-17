--[[
Fixed Window Counter (Quota Tracker) Rate Limiter
using Redis TIME for consistent timing

KEYS[1] = base key (e.g., "quota:{user_id}")
ARGV[1] = limit (maximum requests per window)
ARGV[2] = window (window size in seconds)
ARGV[3] = cost (number of tokens to consume, typically 1)

Returns:
{ allowed (1 or 0), count (current count after increment), ttl_remaining (seconds until window reset) }
]]

local base_key = KEYS[1]
local limit    = tonumber(ARGV[1])
local window   = tonumber(ARGV[2])
local cost     = tonumber(ARGV[3])

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

-- Create a unique key for this specific window
local key = base_key .. ":" .. window .. ":" .. window_start

-- Get current count
local current = redis.call("GET", key)
current = tonumber(current) or 0

-- Check if adding cost would exceed limit
if current + cost > limit then
    -- Denied - return current count without incrementing
    return { 0, current, ttl_remaining }
end

-- Allowed - increment the counter
local new_count = redis.call("INCRBY", key, cost)

-- Set TTL to expire after the window ends (with 1 second buffer)
-- Only set if not already set to avoid resetting TTL on each request
local current_ttl = redis.call("TTL", key)
if current_ttl < 0 then
    redis.call("EXPIRE", key, ttl_remaining + 1)
end

return { 1, new_count, ttl_remaining }
