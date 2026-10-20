-- generate_users.lua

local RECORDS = tonumber(os.getenv("RECORDS")) or 10000
local PASSWORD_LENGTH = tonumber(os.getenv("PASSWORD_LENGTH")) or 12

local names = {
    "Luca", "Giulia", "Marco", "Sofia", "Alessandro", "Francesca", "Matteo", "Chiara", "Davide", "Elena",
    "John", "Emily", "Michael", "Olivia", "James", "Emma", "Daniel", "Sophia", "William", "Ava",
    "Pierre", "Camille", "Louis", "Chloé", "Hugo", "Manon", "Lucas", "Léa", "Nathan", "Inès",
    "Carlos", "María", "Javier", "Lucía", "Miguel", "Carmen", "Diego", "Valentina", "Andrés", "Isabella",
    "João", "Ana", "Pedro", "Beatriz", "Rafael", "Larissa", "Thiago", "Camila", "Felipe", "Mariana",
    "Hans", "Anna", "Lukas", "Mia", "Jonas", "Lea", "Erik", "Sofia", "Nils", "Freja",
    "Ivan", "Anastasia", "Dmitry", "Olga", "Sergei", "Irina", "Mikhail", "Tatiana", "Nikolai", "Svetlana",
    "Ahmed", "Fatima", "Omar", "Aisha", "Youssef", "Layla", "Karim", "Zahra", "Tariq", "Mariam",
    "Noah", "Levi", "Eitan", "Yael", "Ariel", "Shira", "David", "Tamar", "Yonatan", "Michal",
    "Raj", "Priya", "Arjun", "Ananya", "Vikram", "Sneha", "Rohan", "Kavya", "Amit", "Isha"
}

local surnames = {
    "Rossi", "Russo", "Ferrari", "Esposito", "Bianchi", "Romano", "Colombo", "Ricci", "Marino", "Greco",
    "Smith", "Johnson", "Brown", "Taylor", "Anderson", "Thomas", "Jackson", "White", "Harris", "Martin",
    "Dubois", "Moreau", "Laurent", "Simon", "Michel", "Garcia", "Martinez", "Lopez", "Sanchez", "Perez",
    "Rodrigues", "Silva", "Santos", "Oliveira", "Costa", "Ferreira", "Almeida", "Pereira", "Gomes", "Ribeiro",
    "Müller", "Schmidt", "Schneider", "Fischer", "Weber", "Wagner", "Becker", "Hoffmann", "Klein", "Wolf",
    "Ivanov", "Smirnov", "Kuznetsov", "Popov", "Sokolov", "Lebedev", "Kozlov", "Novikov", "Morozov", "Petrov",
    "Haddad", "Khan", "Ali", "Hassan", "Farah", "Rahman", "Aziz", "Mahmoud", "Nasser", "Saleh",
    "Cohen", "Levi", "Mizrahi", "Biton", "Dahan", "Benitez", "Castillo", "Ortega", "Vargas", "Navarro",
    "Kim", "Lee", "Park", "Chen", "Wang", "Li", "Zhang", "Liu", "Tanaka", "Sato",
    "Singh", "Patel", "Sharma", "Gupta", "Das", "Kumar", "Mehta", "Chopra", "Reddy", "Nair"
}

math.randomseed(os.time())

-- Character classes mirroring crates/pw/src/generator.rs, which is what the
-- default Stargate password policy requires (lowercase, uppercase, digit and
-- special character). The comma is omitted from SPECIAL so a password always
-- remains a single CSV field for insert-users.lua.
local UPPERCASE = "ABCDEFGHIJKLMNOPQRSTUVWXYZ"
local LOWERCASE = "abcdefghijklmnopqrstuvwxyz"
local DIGITS = "0123456789"
local SPECIAL = "!@#$%^&*()-_=+[]{}|;:.<>?"

local PASSWORD_CLASSES = { UPPERCASE, LOWERCASE, DIGITS, SPECIAL }
local PASSWORD_CHARSET = table.concat(PASSWORD_CLASSES)

local function random_char(chars)
    local index = math.random(#chars)
    return chars:sub(index, index)
end

local function contains(text, needle)
    return string.find(text, needle, 1, true) ~= nil
end

-- Password guaranteed to satisfy the default policy: one character from every
-- class (when the length allows it) plus no username/email substring.
local function random_password(length, nickname, email)
    local lowered_nickname = string.lower(nickname)
    local lowered_email = string.lower(email)

    local password
    repeat
        local result = {}

        if length >= #PASSWORD_CLASSES then
            for _, class in ipairs(PASSWORD_CLASSES) do
                result[#result + 1] = random_char(class)
            end
        end

        while #result < length do
            result[#result + 1] = random_char(PASSWORD_CHARSET)
        end

        -- Fisher-Yates shuffle: the guaranteed characters must not always be first.
        for i = #result, 2, -1 do
            local j = math.random(i)
            result[i], result[j] = result[j], result[i]
        end

        password = table.concat(result)
    until not contains(string.lower(password), lowered_nickname)
        and not contains(string.lower(password), lowered_email)

    return password
end

local file = assert(io.open("users.csv", "w"))

-- header
file:write("nickname,givenName,familyName,email,password\n")

for i = 1, RECORDS do
    local givenName = names[math.random(#names)]
    local familyName = surnames[math.random(#surnames)]

    local nickname = string.lower(givenName .. familyName .. i)
    local email = string.lower(givenName .. "." .. familyName .. i .. "@example.com")
    local password = random_password(PASSWORD_LENGTH, nickname, email)

    file:write(
        nickname .. "," ..
        givenName .. "," ..
        familyName .. "," ..
        email .. "," ..
        password .. "\n"
    )

    -- stampa progresso ogni 100k
    if i % 100000 == 0 then
        print("Generated " .. i .. " records")
    end
end

file:close()
print("Done. File users.csv generated.")
