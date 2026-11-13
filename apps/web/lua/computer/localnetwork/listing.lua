-- Runs before HappyView stores a computer.localnetwork.listing event: staff listing an act, party or venue, which only
-- the root account writes, from the space on every change. Anyone can write this collection to their own repo, so only
-- the root's get in, and only when every field the directory reads has the type it expects.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local KINDS = { act = true, party = true, venue = true }

local function isPerson(value)
	return type(value) == 'table' and type(value.did) == 'string' and type(value.role) == 'string'
end

-- Whether `value` is a JSON array of people (decoded arrays have only keys 1..n).
local function isPeople(value)
	if type(value) ~= 'table' then return false end
	local count = 0
	for _, item in pairs(value) do
		if not isPerson(item) then return false end
		count = count + 1
	end
	return count == #value
end

-- Why the directory can't read this listing, or nil when it can.
local function problem(value)
	if not KINDS[value.kind] then return 'unknown kind' end
	if type(value.name) ~= 'string' then return 'no name' end
	if not isPeople(value.people) then return 'bad people' end
	if type(value.updatedAt) ~= 'string' then return 'bad updatedAt' end
end

local function gate()
	if did ~= ROOT_DID then
		log('skipped ' .. uri .. ': not the root')
		return nil
	end
	if action == 'delete' then return true end
	local why = problem(record)
	if why then
		log('skipped ' .. uri .. ': ' .. why)
		return nil
	end
	return true
end

function handle()
	local ok, result = pcall(gate)
	if ok then return result end
	log('skipped ' .. uri .. ': ' .. tostring(result))
	return nil
end
