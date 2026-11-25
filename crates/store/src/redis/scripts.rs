use std::sync::LazyLock;

pub static CAS_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| {
    redis::Script::new(
        r#"
        local current = redis.call("GET", KEYS[1])
        if current == ARGV[1] then
            if ARGV[3] ~= "nil" then
                redis.call("SET", KEYS[1], ARGV[2], "EX", tonumber(ARGV[3]))
            else
                redis.call("SET", KEYS[1], ARGV[2])
            end
            return 1
        else
            return 0
        end
        "#,
    )
});

pub static INCR_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| {
    redis::Script::new(
        r#"
        local new_value = redis.call("INCRBY", KEYS[1], ARGV[1])
        if ARGV[2] ~= "nil" then
            redis.call("EXPIRE", KEYS[1], tonumber(ARGV[2]))
        end
        return new_value
        "#,
    )
});

pub static DECR_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| {
    redis::Script::new(
        r#"
        local new_value = redis.call("DECRBY", KEYS[1], ARGV[1])
        if ARGV[2] ~= "nil" then
            redis.call("EXPIRE", KEYS[1], tonumber(ARGV[2]))
        end
        return new_value
        "#,
    )
});

/// HSET + per-field HEXPIRE in one atomic step.
///
/// Returns the HSET result (1 = field was newly created, 0 = overwritten),
/// matching the `Store::hset` contract. HEXPIRE requires Redis >= 7.4; the
/// no-TTL path uses plain HSET and never reaches this script.
pub static HSET_WITH_TTL_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| {
    redis::Script::new(
        r#"
        local added = redis.call("HSET", KEYS[1], ARGV[1], ARGV[2])
        redis.call("HEXPIRE", KEYS[1], tonumber(ARGV[3]), "FIELDS", 1, ARGV[1])
        return added
        "#,
    )
});

pub static CAS_I64_SCRIPT: LazyLock<redis::Script> = LazyLock::new(|| {
    redis::Script::new(
        r#"
        local current = redis.call("GET", KEYS[1])
        if current and tonumber(current) == tonumber(ARGV[1]) then
            if ARGV[3] ~= "nil" then
                redis.call("SET", KEYS[1], ARGV[2], "EX", tonumber(ARGV[3]))
            else
                redis.call("SET", KEYS[1], ARGV[2])
            end
            return 1
        else
            return 0
        end
        "#,
    )
});

/// A [`crate::Transaction`]. KEYS holds the requirement keys, then the write
/// keys. ARGV[1] counts the requirements; then come two arguments per
/// requirement (kind, value) and three per write (kind, value, TTL).
///
/// A deletion that finds nothing still writes, so the WAIT of a confirmed
/// retry covers an earlier deletion that reached only the primary.
pub const TRANSACTION_LUA: &str = r#"
local requirements = tonumber(ARGV[1])
local arg = 2
for i = 1, requirements do
    local kind, value = ARGV[arg], ARGV[arg + 1]
    arg = arg + 2
    if kind == "exists" then
        if redis.call("EXISTS", KEYS[i]) == 0 then
            return 0
        end
    elseif redis.call("GET", KEYS[i]) ~= value then
        return 0
    end
end
for i = requirements + 1, #KEYS do
    local kind, value, ttl = ARGV[arg], ARGV[arg + 1], ARGV[arg + 2]
    arg = arg + 3
    if kind == "set" then
        redis.call("SET", KEYS[i], value, "EX", ttl)
    elseif kind == "replace" then
        redis.call("SET", KEYS[i], value, "KEEPTTL")
    elseif kind == "expire" then
        redis.call("EXPIRE", KEYS[i], ttl)
    elseif redis.call("DEL", KEYS[i]) == 0 then
        redis.call("SET", KEYS[i], "")
        redis.call("DEL", KEYS[i])
    end
end
return 1
"#;

pub static TRANSACTION_SCRIPT: LazyLock<redis::Script> =
    LazyLock::new(|| redis::Script::new(TRANSACTION_LUA));
