-- insert-users.lua
-- Reads users from a CSV file and creates them via the admin API.
-- Usage: lua insert-users.lua
--
-- Requires: lua-socket (luarocks install luasocket)
--           lua-sec    (luarocks install luasec)   -- for HTTPS
--           lua-cjson  (luarocks install lua-cjson)

local http     = require("socket.http")
local https    = require("ssl.https")
local ltn12    = require("ltn12")
local cjson    = require("cjson")
local socket   = require("socket")

-- ---------------------------------------------------------------------------
-- Configuration
-- ---------------------------------------------------------------------------
local BASE_URL = os.getenv("BASE_URL") or "http://localhost:5050/stargate"
local API_KEY  = os.getenv("API_KEY") or "sk_live_c1RCdEZ3c200Q3FqUldYOERsNDlvdmFEMk84eW50NjQ"
local CSV_FILE = os.getenv("CSV_FILE") or "users.csv"
local PAUSE_MS = tonumber(os.getenv("PAUSE_MS")) or 5 -- ms between requests

-- ---------------------------------------------------------------------------
-- Helpers
-- ---------------------------------------------------------------------------
local function sleep_ms(ms)
    socket.sleep(ms / 1000)
end

local function trim(s)
    return s:match("^%s*(.-)%s*$")
end

local function parse_csv_line(line)
    local fields = {}
    for field in line:gmatch("([^,]*)") do
        fields[#fields + 1] = trim(field)
    end
    return fields
end

local function post_json(url, body, api_key)
    local payload = cjson.encode(body)
    local response_body = {}

    local request = http.request
    if url:match("^https") then
        request = https.request
    end

    local _, status, headers = request({
        url     = url,
        method  = "POST",
        headers = {
            ["Content-Type"]   = "application/json",
            ["Content-Length"] = tostring(#payload),
            ["x-api-key"]      = api_key,
        },
        source  = ltn12.source.string(payload),
        sink    = ltn12.sink.table(response_body),
    })

    local resp = table.concat(response_body)
    return status, resp
end

-- ---------------------------------------------------------------------------
-- Main
-- ---------------------------------------------------------------------------
local file = io.open(CSV_FILE, "r")
if not file then
    print("Error: cannot open file " .. CSV_FILE)
    os.exit(1)
end

-- Read header line
local header_line = file:read("*l")
if not header_line then
    print("Error: CSV file is empty")
    os.exit(1)
end

local headers = parse_csv_line(header_line)

-- Build a map of column name -> index
local col = {}
for i, name in ipairs(headers) do
    if name ~= "" then
        col[name] = i
    end
end

-- Validate required columns
if not col["email"] or not col["password"] then
    print("Error: CSV must contain 'email' and 'password' columns")
    os.exit(1)
end

local endpoint = BASE_URL .. "/admin/users"
local total    = 0
local success  = 0
local failed   = 0

print(string.format("Inserting users from '%s' -> %s", CSV_FILE, endpoint))
print(string.format("Pause between requests: %d ms", PAUSE_MS))
print("---")

for line in file:lines() do
    local fields = parse_csv_line(line)

    -- Skip empty lines
    if fields[1] and fields[1] ~= "" then
        total = total + 1

        local body = {
            email      = fields[col["email"]],
            password   = fields[col["password"]],
            givenName  = fields[col["givenName"]],
            familyName = fields[col["familyName"]],
            nickname   = fields[col["nickname"]],
        }

        local status, resp = post_json(endpoint, body, API_KEY)

        if status == 200 or status == 201 then
            success = success + 1
            print(string.format("[%d] OK  %s", total, body.email))
        else
            failed = failed + 1
            print(string.format("[%d] ERR %s - status=%s response=%s", total, body.email, tostring(status), resp))
        end

        if PAUSE_MS > 0 then
            sleep_ms(PAUSE_MS)
        end
    end
end

file:close()

print("---")
print(string.format("Done. total=%d success=%d failed=%d", total, success, failed))
