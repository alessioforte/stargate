--[[
Load Test Script for Multiple Users with JWT Tokens
File: load-test/scripts/multiple-users.lua

Usage:
wrk -t8 -c400 -d30s -s load-test/scripts/multiple-users.lua http://localhost:5050/api/service3/v1/lb
]]

-- Configuration
local config = {
    -- Simulate different users with different JWT tokens
    tokens = {
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJ1c2VyX2lkIjoxLCJyb2xlIjoiYWRtaW4iLCJleHAiOjE3MDAwMDAwMDB9.fake_token_1",
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJ1c2VyX2lkIjoyLCJyb2xlIjoidXNlciIsImV4cCI6MTcwMDAwMDAwMH0.fake_token_2",
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJ1c2VyX2lkIjozLCJyb2xlIjoiZ3Vlc3QiLCJleHAiOjE3MDAwMDAwMDB9.fake_token_3",
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJ1c2VyX2lkIjo0LCJyb2xlIjoidXNlciIsImV4cCI6MTcwMDAwMDAwMH0.fake_token_4",
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJ1c2VyX2lkIjo1LCJyb2xlIjoibW9kIiwiZXhwIjoxNzAwMDAwMDAwfQ.fake_token_5"
    },

    -- Additional headers
    common_headers = {
        ["Content-Type"] = "application/json",
        ["User-Agent"] = "wrk-load-test/1.0",
        ["X-Request-ID"] = ""
    },

    -- Enable/disable debug
    debug = false
}

-- Initialize request counter
local request_counter = 0

--[[
Request function called for every request
]]
request = function()
    request_counter = request_counter + 1

    -- Select random token for this request
    local random_index = math.random(1, #config.tokens)
    local token = config.tokens[random_index]

    -- Prepare headers
    local headers = {}

    -- Copy common headers
    for key, value in pairs(config.common_headers) do
        headers[key] = value
    end

    -- Add authorization header
    headers["Authorization"] = "Bearer " .. token
    headers["X-User-ID"] = tostring(random_index)
    headers["X-Request-ID"] = string.format("req_%d_%d", request_counter, random_index)

    -- Log for debugging
    if config.debug and request_counter <= 10 then
        io.write(string.format("Request #%d - User: %d\n", request_counter, random_index))
    end

    -- Create and return the request
    return wrk.format("GET", wrk.path, headers)
end

--[[
Response function - called for every response
]]
response = function(status, headers, body)
    if status ~= 200 then
        io.write(string.format("Error: HTTP %d - %s\n", status, body))
    end
end

--[[
Done function - called when test completes
]]
done = function(summary, latency, requests)
    io.write("\n" .. string.rep("=", 50) .. "\n")
    io.write("LOAD TEST SUMMARY - MULTIPLE USERS\n")
    io.write(string.rep("=", 50) .. "\n")

    -- Basic metrics
    io.write(string.format("Total Requests:     %d\n", summary.requests))
    io.write(string.format("Duration:           %.2f seconds\n", summary.duration))
    io.write(string.format("Requests/sec:       %.2f\n", summary.requests / summary.duration))
    io.write(string.format("Transfer rate:      %.2f MB/s\n",
        summary.bytes / (1024 * 1024) / summary.duration))

    -- Error reporting
    local total_errors = summary.errors.connect + summary.errors.read +
        summary.errors.write + summary.errors.timeout + summary.errors.status

    if total_errors > 0 then
        io.write(string.format("\nERRORS:\n"))
        io.write(string.format("  Connect:  %d\n", summary.errors.connect))
        io.write(string.format("  Read:     %d\n", summary.errors.read))
        io.write(string.format("  Write:    %d\n", summary.errors.write))
        io.write(string.format("  Timeout:  %d\n", summary.errors.timeout))
        io.write(string.format("  Status:   %d\n", summary.errors.status))
    else
        io.write(string.format("\nErrors:             None ✓\n"))
    end

    -- Latency distribution
    io.write(string.format("\nLATENCY DISTRIBUTION:\n"))
    io.write(string.format("  Average:    %.2f ms\n", latency.mean / 1000))
    io.write(string.format("  Std Dev:    %.2f ms\n", latency.stdev / 1000))
    io.write(string.format("  Max:        %.2f ms\n", latency.max / 1000))
    io.write(string.format("  50%%:        %.2f ms\n", latency:percentile(50) / 1000))
    io.write(string.format("  90%%:        %.2f ms\n", latency:percentile(90) / 1000))
    io.write(string.format("  95%%:        %.2f ms\n", latency:percentile(95) / 1000))
    io.write(string.format("  99%%:        %.2f ms\n", latency:percentile(99) / 1000))

    io.write(string.rep("=", 50) .. "\n")
end

--[[
Setup function - called before test starts
]]
setup = function(thread)
    io.write(string.format("Starting thread %d\n", thread.id))
end
