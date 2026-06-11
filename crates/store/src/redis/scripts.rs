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
