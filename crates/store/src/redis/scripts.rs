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
