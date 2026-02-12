-- generate_users.lua

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

local total_records = 1000000
local file = io.open("users.csv", "w")

-- header
file:write("nickname,givenName,familyName,email,password,\n")

local function random_string(length)
    local chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
    local result = {}
    for i = 1, length do
        local index = math.random(#chars)
        result[i] = chars:sub(index, index)
    end
    return table.concat(result)
end

for i = 1, total_records do
    local givenName = names[math.random(#names)]
    local familyName = surnames[math.random(#surnames)]

    local nickname = string.lower(givenName .. familyName .. i)
    local email = string.lower(givenName .. "." .. familyName .. i .. "@example.com")
    local password = random_string(12)

    file:write(
        nickname .. "," ..
        givenName .. "," ..
        familyName .. "," ..
        email .. "," ..
        password .. ",\n"
    )

    -- stampa progresso ogni 100k
    if i % 100000 == 0 then
        print("Generated " .. i .. " records")
    end
end

file:close()
print("Done. File users.csv generated.")
