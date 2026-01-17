--[[
Token Bucket Rate Limiter
using Redis TIME for consistent timing

KEYS[1] = base key (e.g., "token_bucket:{user_id}")
ARGV[1] = capacity (maximum tokens in bucket)
ARGV[2] = refill_rate (tokens added per second)
ARGV[3] = cost (tokens to consume)
ARGV[4] = ttl (key expiration in seconds)

Returns:
{ allowed (1 or 0), remaining (tokens after operation), retry_after_ms (milliseconds until enough tokens) }

The bucket stores two values:
- {key}:tokens - current token count (as integer, scaled by 1000 for precision)
- {key}:time - last update timestamp in microseconds
]]

local base_key    = KEYS[1]
local capacity    = tonumber(ARGV[1])
local refill_rate = tonumber(ARGV[2])
local cost        = tonumber(ARGV[3])
local ttl         = tonumber(ARGV[4])

-- Validate arguments
if not capacity or capacity <= 0 then
    return redis.error_reply("Invalid capacity argument")
end

if not refill_rate or refill_rate < 0 then
    return redis.error_reply("Invalid refill_rate argument")
end

if not cost or cost < 0 then
    return redis.error_reply("Invalid cost argument")
end

if not ttl or ttl <= 0 then
    return redis.error_reply("Invalid ttl argument")
end

-- Keys for storing state
local tokens_key = base_key .. ":tokens"
local time_key = base_key .. ":time"

-- Get Redis time (seconds, microseconds)
local time_parts = redis.call("TIME")
if not time_parts or #time_parts < 2 then
    return redis.error_reply("Failed to get Redis TIME")
end

local now_micros = (tonumber(time_parts[1]) * 1000000) + tonumber(time_parts[2])

-- Get current state
local stored_tokens = redis.call("GET", tokens_key)
local stored_time = redis.call("GET", time_key)

local current_tokens
local last_update

if stored_tokens and stored_time then
    -- We store tokens scaled by 1000 for sub-token precision
    current_tokens = tonumber(stored_tokens)
    last_update = tonumber(stored_time)
else
    -- Initialize with full bucket (scaled by 1000)
    current_tokens = capacity * 1000
    last_update = now_micros
end

-- Calculate tokens to add based on elapsed time
local elapsed_micros = now_micros - last_update
if elapsed_micros < 0 then
    elapsed_micros = 0
end

-- tokens_to_add = elapsed_seconds * refill_rate
-- elapsed_seconds = elapsed_micros / 1_000_000
-- We scale by 1000, so: tokens_to_add_scaled = (elapsed_micros / 1_000_000) * refill_rate * 1000
--                                            = elapsed_micros * refill_rate / 1000
local tokens_to_add = math.floor((elapsed_micros * refill_rate) / 1000)

-- Calculate available tokens (capped at capacity, still scaled by 1000)
local capacity_scaled = capacity * 1000
local available_tokens = current_tokens + tokens_to_add
if available_tokens > capacity_scaled then
    available_tokens = capacity_scaled
end

-- Cost scaled by 1000
local cost_scaled = cost * 1000

-- Check if we have enough tokens
if available_tokens >= cost_scaled then
    -- Allowed - consume tokens
    local new_tokens = available_tokens - cost_scaled

    -- Update state
    redis.call("SET", tokens_key, tostring(new_tokens), "EX", ttl)
    redis.call("SET", time_key, tostring(now_micros), "EX", ttl)

    -- Return remaining tokens (unscaled)
    local remaining = math.floor(new_tokens / 1000)
    return { 1, remaining, 0 }
else
    -- Denied - not enough tokens
    -- Update state to track refill progress
    redis.call("SET", tokens_key, tostring(available_tokens), "EX", ttl)
    redis.call("SET", time_key, tostring(now_micros), "EX", ttl)

    -- Calculate retry_after in milliseconds
    local tokens_needed = cost_scaled - available_tokens
    local retry_after_ms
    if refill_rate > 0 then
        -- tokens_needed is scaled by 1000
        -- retry_ms = tokens_needed / (refill_rate * 1000) * 1000 = tokens_needed / refill_rate
        retry_after_ms = math.ceil(tokens_needed / refill_rate)
    else
        retry_after_ms = 0xFFFFFFFF -- Max uint32 if no refill
    end

    -- Return remaining tokens (unscaled)
    local remaining = math.floor(available_tokens / 1000)
    return { 0, remaining, retry_after_ms }
end
