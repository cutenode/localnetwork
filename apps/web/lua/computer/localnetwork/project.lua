-- Runs before HappyView stores a computer.localnetwork.project event. Jetstream filters by collection, not by
-- account, and anyone can write this collection to their own repo, so only the root account's records get in.
-- HappyView stores a record anyway when this script errors, so it checks types instead of trusting them.

local STATUSES = { active = true, hidden = true, archived = true }

local function isString(value)
	return type(value) == 'string'
end

local function isInteger(value)
	return math.type(value) == 'integer'
end

local function isObject(value)
	return type(value) == 'table'
end

-- A JSON array of strings (decoded arrays have only keys 1..n).
local function isStrings(value)
	if type(value) ~= 'table' then return false end
	local count = 0
	for _, item in pairs(value) do
		if type(item) ~= 'string' then return false end
		count = count + 1
	end
	return count == #value
end

local function optional(check, value)
	return value == nil or check(value)
end

local function isFee(value)
	return isObject(value) and optional(isString, value.tier)
end

-- Why the directory can't serve this record, or nil when every field its queries read has the type they expect.
local function problem(value)
	local profile = isObject(value) and value.profile
	local location = isObject(profile) and profile.location
	if not isObject(location) then return 'no profile.location' end
	if not (isString(value.slug) and STATUSES[value.status] and isStrings(value.managers)) then
		return 'bad slug, status, or managers'
	end
	if not (isString(value.createdAt) and isString(value.updatedAt)) then return 'bad timestamps' end
	if not (isString(profile.name) and isString(profile.kind) and isStrings(profile.genres)) then
		return 'bad name, kind, or genres'
	end
	if not (isString(location.city) and isString(location.country) and optional(isString, location.region)) then
		return 'bad place'
	end
	if not (optional(isInteger, location.latE6) and optional(isInteger, location.lngE6)) then return 'bad coordinates' end
	for _, field in ipairs({ 'tagline', 'bio', 'lookingFor' }) do
		if not optional(isString, profile[field]) then return 'bad ' .. field end
	end
	for _, field in ipairs({ 'highlights', 'formats', 'setTypes' }) do
		if not optional(isStrings, profile[field]) then return 'bad ' .. field end
	end
	local travel, fees, capacity = profile.travel or {}, profile.fees or {}, profile.capacity or {}
	if not (isObject(travel) and optional(isInteger, travel.radiusKm)) then return 'bad travel' end
	if not (isObject(fees) and optional(isFee, profile.fee)) then return 'bad fee' end
	for _, fee in ipairs(fees) do
		if not isFee(fee) then return 'bad fee' end
	end
	if not (isObject(capacity) and optional(isInteger, capacity.min) and optional(isInteger, capacity.max)) then
		return 'bad capacity'
	end
end

function handle()
	if did ~= ROOT_DID then return nil end
	if action == 'delete' then return true end
	local why = problem(record)
	if why then
		log('skipped ' .. uri .. ': ' .. why)
		return nil
	end
	return true
end
