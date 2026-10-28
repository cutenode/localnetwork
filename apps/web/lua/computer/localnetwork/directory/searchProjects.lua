-- Search published projects. SQL applies the cheap filters (status, place, kind, fee tier, managers, a bounding
-- box, and a substring prefilter per search word); Lua applies genres, formats, set types, exact distance, and
-- word-prefix matching, then ranks and pages.

local BY_NAME = " ORDER BY lower(json_extract(record, '$.profile.name')), json_extract(record, '$.profile.name'), uri"

-- Up to 8 lowercase words from `q`. UTF-8 bytes count as word characters, so accented words stay whole.
local function terms(q)
	local out = {}
	for term in q:lower():gmatch('[%w\128-\255]+') do
		out[#out + 1] = term
		if #out == 8 then break end
	end
	return out
end

-- `value` as lowercase words, each preceded by a space, so ' ' .. term finds a word starting with term.
local function words(value)
	return ' ' .. value:lower():gsub('[^%w\128-\255]+', ' ')
end

-- How well a profile matches `needles` (' ' .. term each), weighting fields like the old full-text index did
-- (name 10, tagline 4, about 1, genres 6, place 3) for every term found. nil when some term matches nowhere.
local function relevance(profile, needles)
	local location = profile.location
	local highlights = table.concat(profile.highlights or {}, ' ')
	local fields = {
		{ 10, words(profile.name) },
		{ 4, words(profile.tagline or '') },
		{ 1, words((profile.bio or '') .. ' ' .. (profile.lookingFor or '') .. ' ' .. highlights) },
		{ 6, words(table.concat(profile.genres, ' ')) },
		{ 3, words(location.city .. ' ' .. (location.region or '') .. ' ' .. location.country) },
	}
	local total = 0
	for _, needle in ipairs(needles) do
		local found = 0
		for _, field in ipairs(fields) do
			if field[2]:find(needle, 1, true) then found = found + field[1] end
		end
		if found == 0 then return nil end
		total = total + found
	end
	return total
end

-- True when nothing is wanted or `offered` (a list, maybe missing) shares a value with `wanted`.
local function overlaps(wanted, offered)
	if #wanted == 0 then return true end
	for _, value in ipairs(offered or {}) do
		if contains(wanted, value) then return true end
	end
	return false
end

function handle()
	local where, args = {}, {}
	local function filter(clause, values)
		where[#where + 1] = ' AND ' .. clause
		for _, value in ipairs(values or {}) do args[#args + 1] = value end
	end

	local status = text(params.status)
	if status then
		filter("json_extract(record, '$.status') = ?", { status })
	elseif params.includeInactive ~= true then
		filter("json_extract(record, '$.status') = 'active'")
	end
	local country = text(params.country)
	if country then filter("upper(json_extract(record, '$.profile.location.country')) = upper(?)", { country }) end
	local kinds, tiers = list(params.kinds, 10), list(params.feeTiers, 10)
	if #kinds > 0 then filter("json_extract(record, '$.profile.kind') IN (" .. marks(#kinds) .. ')', kinds) end
	if #tiers > 0 then
		-- Any fee level's tier, or the one fee of a record written before `fees`.
		local inTiers, both = ' IN (' .. marks(#tiers) .. ')', {}
		for i, tier in ipairs(tiers) do both[i], both[#tiers + i] = tier, tier end
		filter(
			"(EXISTS (SELECT 1 FROM json_each(record, '$.profile.fees') WHERE json_extract(value, '$.tier')" .. inTiers
				.. ") OR json_extract(record, '$.profile.fee.tier')" .. inTiers .. ')',
			both
		)
	end
	if params.unclaimed == true then filter("json_array_length(record, '$.managers') = 0") end
	local manager = text(params.manager)
	if manager then filter("EXISTS (SELECT 1 FROM json_each(record, '$.managers') WHERE value = ?)", { manager }) end

	local needles = {}
	for _, term in ipairs(terms(text(params.q) or '')) do
		needles[#needles + 1] = ' ' .. term
		-- Every word appears somewhere in the record's JSON, a superset of the word-prefix match below.
		filter('record LIKE ?', { '%' .. term .. '%' })
	end

	local near = point(params.latE6, params.lngE6)
	local radius = integer(params.radiusKm) or 50
	if near then filter(withinBox(near, radius, 'record', '$.profile.location')) end

	local close = {}
	for _, want in ipairs(normalizeGenres(list(params.genres, 20), 20)) do
		for genre, value in pairs(similarities(want)) do
			if value >= 0.6 then close[genre] = true end
		end
	end
	local wantsGenres = next(close) ~= nil
	local formats, setTypes = list(params.formats, 10), list(params.setTypes, 10)
	local sort = text(params.sort) or (#needles > 0 and 'relevance') or (near and 'distance') or 'recent'

	-- The filters SQL can't apply, short of distance and search words.
	local function wanted(profile)
		if not (overlaps(formats, profile.formats) and overlaps(setTypes, profile.setTypes)) then return false end
		if not wantsGenres then return true end
		for _, genre in ipairs(normalizeGenres(profile.genres, 12)) do
			if close[genre] then return true end
		end
		return false
	end

	local items = {}
	for i, project in ipairs(projects(table.concat(where), args, sort == 'name' and BY_NAME or nil)) do
		local profile = project.record.profile
		if wanted(profile) then
			local km = near and distanceKm(near, profile.location.latE6, profile.location.lngE6)
			local rank = (not km or km <= radius) and (#needles == 0 or relevance(profile, needles))
			if rank then
				project.distanceM = km and meters(km)
				items[#items + 1] = { project = project, km = km, rank = rank, i = i }
			end
		end
	end

	-- SQL already sorted by name or newest first; ties keep that order.
	if sort == 'relevance' and #needles > 0 then
		table.sort(items, function(a, b)
			if a.rank ~= b.rank then return a.rank > b.rank end
			return a.i < b.i
		end)
	elseif sort == 'distance' then
		table.sort(items, function(a, b)
			local x, y = a.km or math.huge, b.km or math.huge
			if x ~= y then return x < y end
			return a.i < b.i
		end)
	end

	local limit = pageSize(params.limit, 24, 60)
	local offset = tonumber(text(params.cursor) or '')
	offset = math.type(offset) == 'integer' and offset > 0 and offset or 0
	local page = {}
	for i = offset + 1, math.min(#items, offset + limit) do page[#page + 1] = items[i].project end
	return {
		projects = toarray(page),
		total = #items,
		cursor = offset + limit < #items and tostring(offset + limit) or nil,
	}
end
