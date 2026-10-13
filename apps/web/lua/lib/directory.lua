-- Shared by every directory script. The bundle (src/lib/server/happyview/bundle.ts) puts this after the prelude
-- that defines ROOT_DID and TAXONOMY and before the script itself, all in one chunk, so these locals are in scope
-- there. HappyView runs each request in a fresh VM, and runs top-level code at upload too, without `db` and under
-- its 1,000,000-instruction limit, so top-level code only builds lookups. handle() is held to the same budget by
-- the tests, which is why SQL narrows the rows before Lua touches them.

local PROJECT = 'computer.localnetwork.project'

local SET = 'computer.localnetwork.dj.set'

-- The mean Earth radius the app's distances have always used.
local EARTH_RADIUS_KM = 6371.0088

-- Newest first, then by URI so ties have a stable order.
local RECENT = " ORDER BY json_extract(record, '$.createdAt') DESC, uri"

-- Params ----------------------------------------------------------------------------------------------------------

-- HappyView passes a param given once as a string and a repeated one as a list. It doesn't enforce the lexicon's
-- maxLength, so the list is cut to `max` here.
local function list(value, max)
	if value == nil then return {} end
	if type(value) ~= 'table' then return { value } end
	return table.move(value, 1, math.min(#value, max), 1, {})
end

local function contains(values, wanted)
	for _, value in ipairs(values) do
		if value == wanted then return true end
	end
	return false
end

-- A string param, or nil when it's missing, empty, or repeated.
local function text(value)
	return type(value) == 'string' and value ~= '' and value or nil
end

-- An integer param (HappyView coerces those the lexicon declares), or nil.
local function integer(value)
	return math.type(value) == 'integer' and value or nil
end

-- A page size: `default` when missing, otherwise clamped to 1..max.
local function pageSize(value, default, max)
	return math.max(1, math.min(max, integer(value) or default))
end

-- SQL placeholders for an IN list of `count` values.
local function marks(count)
	return string.rep('?', count, ', ')
end

-- Geography -------------------------------------------------------------------------------------------------------

local sin, cos, asin, sqrt, PI = math.sin, math.cos, math.asin, math.sqrt, math.pi

-- The searched place in degrees from E6 integer params, with its latitude's cosine for distanceKm. nil unless
-- both are integers.
local function point(latE6, lngE6)
	if math.type(latE6) ~= 'integer' or math.type(lngE6) ~= 'integer' then return nil end
	local lat = latE6 / 1e6
	return { lat = lat, lng = lngE6 / 1e6, cos = cos(lat * PI / 180) }
end

-- Great-circle kilometres (haversine) from `from` (a point) to a record's latE6/lngE6. It runs once per project, so
-- it takes the record's integers as they are.
local function distanceKm(from, latE6, lngE6)
	local lat, lng = latE6 / 1e6, lngE6 / 1e6
	local sinLat, sinLng = sin((lat - from.lat) * PI / 180 / 2), sin((lng - from.lng) * PI / 180 / 2)
	local h = sinLat * sinLat + from.cos * cos(lat * PI / 180) * (sinLng * sinLng)
	return 2 * EARTH_RADIUS_KM * asin(math.min(1, sqrt(h)))
end

local function meters(km)
	return math.floor(km * 1000 + 0.5)
end

-- Records ---------------------------------------------------------------------------------------------------------

-- The root account's project records, as projectViews ({ uri, cid, record }), narrowed by `where` (SQL starting
-- with ' AND ', its placeholders filled from `args`) and sorted by `order` (newest first by default).
local function projects(where, args, order)
	local values = { PROJECT, ROOT_DID }
	for _, arg in ipairs(args) do values[#values + 1] = arg end
	local rows = db.raw(
		'SELECT uri, cid, record FROM happyview_records WHERE collection = ? AND did = ?' .. where .. (order or RECENT),
		values
	)
	for _, row in ipairs(rows) do row.record = json.decode(row.record) end
	return rows
end

-- Sets as setViews: the record script (computer/localnetwork/dj/set.lua) stores each one's view fields, so this
-- adds the URI and CID. Narrowed and sorted like projects(), with `order` required.
local function sets(where, args, order)
	local values = { SET }
	for _, arg in ipairs(args) do values[#values + 1] = arg end
	local rows = db.raw('SELECT uri, cid, record FROM happyview_records WHERE collection = ?' .. where .. order, values)
	local out = {}
	for i, row in ipairs(rows) do
		local set = json.decode(row.record)
		set.uri, set.cid = row.uri, row.cid
		out[i] = set
	end
	return toarray(out)
end

-- Genres ----------------------------------------------------------------------------------------------------------

-- Family by genre, then by family id, so a family's id (like 'bass') finds it even when it isn't one of its genres.
local FAMILY = {}
for _, family in ipairs(TAXONOMY.families) do
	for _, genre in ipairs(family.genres) do FAMILY[genre] = family end
end
for _, family in ipairs(TAXONOMY.families) do
	FAMILY[family.id] = FAMILY[family.id] or family
end

-- Pairs of families that share audiences, keyed both ways round.
local NEAR = {}
for _, family in ipairs(TAXONOMY.families) do
	for _, id in ipairs(family.near or {}) do
		NEAR[family.id .. '|' .. id], NEAR[id .. '|' .. family.id] = true, true
	end
end

local normalized = {}

-- Normalized genres without blanks, duplicates, or anything over 48 bytes, at most `max` of them. Normalizing works
-- like normalizeGenre in src/lib/taxonomy/genres.ts minus NFKC and non-ASCII lowercasing, which Lua lacks; callers
-- normalize with the TypeScript first, so this only has to agree on what that produces. Memoized per input (false
-- for one that normalizes to nothing usable), since projects share genres.
local function normalizeGenres(inputs, max)
	local out, seen = {}, {}
	for i = 1, #inputs do
		local input = inputs[i]
		local genre = normalized[input]
		if genre == nil then
			genre = input:lower():gsub('[%s_]+', ' '):match('^ ?(.-) ?$')
			genre = TAXONOMY.aliases[genre] or genre
			if genre == '' or #genre > 48 then genre = false end
			normalized[input] = genre
		end
		if genre and not seen[genre] then
			seen[genre] = true
			out[#out + 1] = genre
			if #out == max then break end
		end
	end
	return out
end

-- How alike two normalized genres are: 1 when equal, 0.75 for a family's headline genre (or id) against another
-- of its genres, 0.6 for two other genres of one family, 0.3 across neighbouring families, otherwise 0.
local function similarity(left, right)
	if left == right then return 1 end
	local a, b = FAMILY[left], FAMILY[right]
	if not a or not b then return 0 end
	if a == b then
		local head = a.genres[1]
		return (left == a.id or right == a.id or left == head or right == head) and 0.75 or 0.6
	end
	return NEAR[a.id .. '|' .. b.id] and 0.3 or 0
end

-- Every genre's similarity to `wanted`, leaving out zeros, so comparing a project's genres is a lookup each.
local function similarities(wanted)
	local out = { [wanted] = 1 }
	for genre in pairs(FAMILY) do
		local value = similarity(wanted, genre)
		if value > 0 then out[genre] = value end
	end
	return out
end
