-- Runs before HappyView stores a computer.localnetwork.profile event. Every account's profile is kept, at the key
-- `self`, since anyone can have a profile page; searchPerformers only reads the ones the root lists. A profile is kept
-- as written, but only when every field the site reads has the type it expects.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local function isString(value)
	return type(value) == 'string'
end

local function isInteger(value)
	return math.type(value) == 'integer'
end

local function isObject(value)
	return type(value) == 'table'
end

local function optional(check, value)
	return value == nil or check(value)
end

-- A JSON array whose items all pass `check` (decoded arrays have only keys 1..n).
local function isList(check)
	return function(value)
		if not isObject(value) then return false end
		local count = 0
		for _, item in pairs(value) do
			if not check(item) then return false end
			count = count + 1
		end
		return count == #value
	end
end

local isStrings = isList(isString)

local function isLocation(value)
	return isObject(value) and isString(value.city) and isString(value.country) and optional(isString, value.region)
		and optional(isInteger, value.latE6) and optional(isInteger, value.lngE6)
end

local function isLink(value)
	return isObject(value) and isString(value.kind) and isString(value.url) and optional(isString, value.label)
end

local function isBlob(value)
	return isObject(value) and isObject(value.ref) and isString(value.ref['$link']) and isString(value.mimeType)
end

local function isFee(value)
	return isObject(value) and optional(isString, value.tier)
end

local function isMix(value)
	return isObject(value) and isString(value.title) and isString(value.url)
end

-- An object whose named fields, when present, are integers.
local function hasIntegers(...)
	local fields = { ... }
	return function(value)
		if not isObject(value) then return false end
		for _, field in ipairs(fields) do
			if not optional(isInteger, value[field]) then return false end
		end
		return true
	end
end

-- Why the site can't read this profile, or nil when every field it reads has the type it expects.
local function problem(value)
	local basics = value.basics
	if not (isObject(basics) and isString(basics.name)) then return 'no basics.name' end
	if not (isString(value.createdAt) and isString(value.updatedAt)) then return 'bad timestamps' end
	if not (optional(isString, basics.bio) and optional(isString, basics.pronouns)) then return 'bad bio or pronouns' end
	if not optional(isLocation, basics.location) then return 'bad location' end
	if not optional(isList(isLink), basics.links) then return 'bad links' end
	if not optional(isBlob, basics.avatar) then return 'bad avatar' end

	local performer = value.performer
	if performer ~= nil then
		if not isObject(performer) then return 'bad performer' end
		for _, field in ipairs({ 'tagline', 'lookingFor' }) do
			if not optional(isString, performer[field]) then return 'bad ' .. field end
		end
		for _, field in ipairs({ 'genres', 'formats', 'setTypes', 'highlights' }) do
			if not optional(isStrings, performer[field]) then return 'bad ' .. field end
		end
		if not optional(isList(isFee), performer.fees) then return 'bad fees' end
		if not optional(isList(isMix), performer.mixes) then return 'bad mixes' end
		if not optional(isInteger, performer.activeSince) then return 'bad activeSince' end
		if not optional(hasIntegers('radiusKm'), performer.travel) then return 'bad travel' end
		if not optional(hasIntegers('min', 'max'), performer.capacity) then return 'bad capacity' end
		if not optional(hasIntegers('minMinutes', 'maxMinutes'), performer.setLength) then return 'bad setLength' end
	end

	local promoter = value.promoter
	if promoter ~= nil and not (isObject(promoter) and optional(isList(isLocation), promoter.cities)) then
		return 'bad promoter'
	end
end

local function gate()
	if rkey ~= 'self' then return nil end
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
