--[[
Lua script implementing token bucket + optional quota.
KEYS[1] = bucket key (hash)
KEYS[2] = quota key (string)
ARGV[1] = capacity (integer)
ARGV[2] = refill_per_sec (float)
ARGV[3] = now (float seconds)
ARGV[4] = cost (integer)
ARGV[5] = bucket_ttl (seconds)
ARGV[6] = quota_ttl (seconds)
ARGV[7] = quota_allowance (-1 => don't check quota, otherwise integer)

Returns array: {allowed (1/0), tokens_after, quota_after (or -1), reason}a
]]

-- fetch args
local capacity = tonumber(ARGV[1])
local refill_per_sec = tonumber(ARGV[2])
local now = tonumber(ARGV[3])
local cost = tonumber(ARGV[4])
local bucket_ttl = tonumber(ARGV[5])
local quota_ttl = tonumber(ARGV[6])
local quota_allowed = tonumber(ARGV[7])

-- KEYS
local bucket_key = KEYS[1]
local quota_key = KEYS[2]

-- read bucket state
local exists = redis.call("EXISTS", bucket_key)
local tokens = 0
local last_ts = now
if exists == 1 then
    local t = redis.call("HMGET", bucket_key, "tokens", "ts")
    tokens = tonumber(t[1]) or 0
    last_ts = tonumber(t[2]) or now
end

-- compute refill (allow fractional)
local delta = math.max(0, now - last_ts)
local added = delta * refill_per_sec
tokens = math.min(capacity, tokens + added)

-- decide if allowed by tokens
if tokens + 0.0000001 < cost then
    -- not enough tokens
    return {0, tostring(tokens), -1, "insufficient_tokens"}
end

-- deduct tokens
tokens = tokens - cost

-- persist bucket
redis.call("HMSET", bucket_key, "tokens", tostring(tokens), "ts", tostring(now))
redis.call("EXPIRE", bucket_key, bucket_ttl)

-- handle quota if requested
if quota_allowed >= 0 then
    local quota = tonumber(redis.call("GET", quota_key) or "0")
    if quota < cost then
        -- insufficient quota
        return {0, tostring(tokens), quota, "insufficient_quota"}
    end
    quota = quota - cost
    redis.call("SET", quota_key, tostring(quota))
    redis.call("EXPIRE", quota_key, quota_ttl)
    return {1, tostring(tokens), quota, "ok"}
else
    -- no quota tracking
    return {1, tostring(tokens), -1, "ok"}
end
