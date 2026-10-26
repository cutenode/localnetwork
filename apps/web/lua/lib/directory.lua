-- Shared by every directory script. The bundle (src/lib/server/happyview/bundle.ts) puts this after the prelude
-- that defines ROOT_DID and TAXONOMY and before the script itself, all in one chunk, so these locals are in scope
-- there. HappyView runs each request in a fresh VM, and runs top-level code at upload too, without `db` and under
-- its 1,000,000-instruction limit, so top-level code only builds lookups. handle() is held to the same budget by
-- the tests, which is why SQL narrows the rows before Lua touches them.

local PROJECT = 'computer.localnetwork.project'

local SET = 'computer.localnetwork.dj.set'

local SHOW = 'computer.localnetwork.show'

local FOLLOW = 'computer.localnetwork.follow'

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

-- A lat/lng box around every point within `radiusKm` of `center`, for a cheap SQL prefilter before exact distances.
local function boundingBox(center, radiusKm)
	local latDelta = radiusKm / 111.32
	local lngDelta = center.cos < 1e-6 and 180 or math.min(180, radiusKm / (111.32 * center.cos))
	return {
		minLat = math.max(-90, center.lat - latDelta),
		maxLat = math.min(90, center.lat + latDelta),
		minLng = center.lng - lngDelta,
		maxLng = center.lng + lngDelta,
	}
end

-- An SQL condition keeping the latE6 and lngE6 under `location` (a JSON path into `column`) inside a boundingBox
-- around `center`, and its placeholders' values. A box that crosses the antimeridian takes longitudes on both sides.
local function withinBox(center, radiusKm, column, location)
	local box = boundingBox(center, radiusKm)
	local lat = 'json_extract(' .. column .. ", '" .. location .. ".latE6')"
	local lng = 'json_extract(' .. column .. ", '" .. location .. ".lngE6')"
	local clause = lat .. ' BETWEEN ? AND ?'
	local values = { math.floor(box.minLat * 1e6), math.ceil(box.maxLat * 1e6) }
	local ranges = { { box.minLng, box.maxLng } }
	if box.maxLng - box.minLng >= 360 then
		return clause, values
	elseif box.minLng < -180 then
		ranges = { { box.minLng + 360, 180 }, { -180, box.maxLng } }
	elseif box.maxLng > 180 then
		ranges = { { box.minLng, 180 }, { -180, box.maxLng - 360 } }
	end
	local either = {}
	for i, range in ipairs(ranges) do
		either[i] = lng .. ' BETWEEN ? AND ?'
		values[#values + 1] = math.floor(range[1] * 1e6)
		values[#values + 1] = math.ceil(range[2] * 1e6)
	end
	return clause .. ' AND (' .. table.concat(either, ' OR ') .. ')', values
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

-- A happyview_records row (uri, cid, and record JSON) as a recordView, its value as its record script stored it.
local function recordView(row)
	return { uri = row.uri, cid = row.cid, value = json.decode(row.record) }
end

-- A stored set's view fields (as JSON) as a setView, with `uri` and `cid`.
local function setView(record, uri, cid)
	local set = json.decode(record)
	set.uri, set.cid = uri, cid
	return set
end

-- SQL for a stored set's view fields: its record without the tracklist, who it was played with, where to listen and
-- its event, which only the DJ sets block reads, or the named tracks the support queries count.
local SET_FIELDS = "json_remove(record, '$.tracks', '$.playedWith', '$.links', '$.event', '$.namedTracks')"
	.. ' AS record'

-- Sets as setViews: the record script (computer/localnetwork/dj/set.lua) stores each one's view fields, tracklist,
-- and named tracks. It keeps every account's, so only sets by accounts that manage one of the root's projects, in any
-- status, are read here. Narrowed and sorted like projects(), with `order` required.
local function sets(where, args, order)
	local values = { SET, PROJECT, ROOT_DID }
	for _, arg in ipairs(args) do values[#values + 1] = arg end
	local rows = db.raw(
		'SELECT uri, cid, ' .. SET_FIELDS .. ' FROM happyview_records AS s WHERE s.collection = ?'
			.. " AND s.did IN (SELECT m.value FROM happyview_records AS p, json_each(p.record, '$.managers') AS m"
			.. ' WHERE p.collection = ? AND p.did = ?)'
			.. where
			.. order,
		values
	)
	local out = {}
	for i, row in ipairs(rows) do out[i] = setView(row.record, row.uri, row.cid) end
	return toarray(out)
end

-- Shows -------------------------------------------------------------------------------------------------------------

-- SQL (starting with ' AND ') for a show, aliased s, that hasn't ended. One without an end stays on for 12 hours after
-- it starts, through the night. datetime() reads each time's UTC offset, so this compares instants.
local UPCOMING = " AND coalesce(datetime(json_extract(s.record, '$.endsAt')),"
	.. " datetime(json_extract(s.record, '$.startsAt'), '+12 hours')) > datetime('now')"

-- Shows (aliased s) by a current manager of the active root project they name (aliased p), narrowed by `where` (SQL
-- starting with ' AND ') and followed by `order` (ORDER BY and LIMIT, or ''), their placeholders filled from `args`.
-- Each row has the show's uri, cid, and record JSON, its latE6 and lngE6, and the project's projectUri, projectCid,
-- and project JSON, so callers decode only the rows they return.
local function showRows(where, args, order)
	local values = { SHOW, PROJECT, ROOT_DID }
	for _, arg in ipairs(args) do values[#values + 1] = arg end
	return db.raw(
		'SELECT s.uri, s.cid, s.record,'
			.. " json_extract(s.record, '$.location.latE6') AS latE6, json_extract(s.record, '$.location.lngE6') AS lngE6,"
			.. ' p.uri AS projectUri, p.cid AS projectCid, p.record AS project'
			.. " FROM happyview_records s JOIN happyview_records p ON p.uri = json_extract(s.record, '$.project')"
			.. " WHERE s.collection = ? AND p.collection = ? AND p.did = ? AND json_extract(p.record, '$.status') = 'active'"
			.. " AND EXISTS (SELECT 1 FROM json_each(p.record, '$.managers') WHERE value = s.did)"
			.. where
			.. order,
		values
	)
end

-- A showRows row as a showView, `km` from the searched place when there was one.
local function showView(row, km)
	return {
		uri = row.uri,
		cid = row.cid,
		record = json.decode(row.record),
		project = { uri = row.projectUri, cid = row.projectCid, record = json.decode(row.project) },
		distanceM = km and meters(km),
	}
end

-- Supports --------------------------------------------------------------------------------------------------------

-- How the support queries count, after 1001Tracklists' Top 101 Producers: a DJ supports a track once however often
-- they play it, an artist gets at most 5 supports from one DJ, and a track charts once 3 DJs support it. A DJ is an
-- act: a project, so a crew whose managers each publish the same gig counts once.
local MAX_SUPPORTS_PER_DJ = 5

local MIN_DJS_TO_CHART = 3

local UUID = '^' .. string.rep('%x', 8) .. string.rep('%-' .. string.rep('%x', 4), 3) .. '%-'
	.. string.rep('%x', 12) .. '$'

-- Like the record script's normalize(), which made the keys these queries compare against. ASCII-only lowercasing,
-- as Lua has no Unicode case tables, so "Ä" and "ä" stay two names.
local function normalize(name)
	return (name:lower():gsub('%s+', ' '):match('^ ?(.-) ?$'))
end

-- A MusicBrainz ID param (`mbid:<uuid>` or the bare UUID) as the bare lowercase UUID the record script stores, or
-- nil when it isn't one.
local function mbid(value)
	return (value:lower():gsub('^mbid:', '')):match(UUID)
end

-- The accounts that manage an active root project, each with their act: the first such project (by URI), so a
-- crew's managers count as one DJ. Checked on every query, since the record script only checks when a set arrives.
-- Its placeholders are the project collection and the root DID.
local DJS = [[djs AS MATERIALIZED (
	SELECT m.value AS did, min(p.uri) AS act FROM happyview_records AS p, json_each(p.record, '$.managers') AS m
	WHERE p.collection = ? AND p.did = ? AND json_extract(p.record, '$.status') = 'active'
	GROUP BY m.value
)]]

-- Every named track of the current DJs' sets as a play: a row per play and artist it credits, with the track and
-- the artist it counts for. A track is its recording MBID, else its ISRC, else its credit and title; an artist is
-- their MBID, else their name. A play without an ID takes the one other plays have when only one fits (an ISRC's
-- recording, a title's recording or ISRC, a name's artist), so sets published before their writer filled in IDs
-- count with the ones after. It reads every set each time, which is why each query builds it once (MATERIALIZED).
-- Each set's named tracks are pulled out of its record once, and each play keeps its track's JSON (`track`), since
-- going back to a record for a track means SQLite parsing the whole record, tracklist and all, again: dozens of
-- times slower over long sets. playsArgs() gives its placeholders.
local PLAYS = 'WITH ' .. DJS .. [[, sets AS (
	SELECT s.uri, s.did, d.act, json_extract(s.record, '$.namedTracks') AS named,
		json_extract(s.record, '$.date') AS date,
		json_extract(s.record, '$.location.locality') AS city, upper(json_extract(s.record, '$.location.country')) AS country
	FROM happyview_records AS s JOIN djs AS d ON d.did = s.did WHERE s.collection = ?
), credits AS MATERIALIZED (
	SELECT s.uri AS setUri, t.key AS position, s.did, s.act, s.date, s.city, s.country,
		json_extract(t.value, '$.startSeconds') AS startSeconds, json_extract(t.value, '$.recordingMbId') AS mbid,
		json_extract(t.value, '$.isrc') AS isrc, json_extract(t.value, '$.key') AS name,
		json_extract(a.value, '$.artistMbId') AS artistMbid, json_extract(a.value, '$.key') AS artistKey,
		json_extract(a.value, '$.artistName') AS artistName, t.value AS track
	FROM sets AS s, json_each(s.named) AS t, json_each(t.value, '$.artists') AS a
), isrcMbids AS (
	SELECT isrc, min(mbid) AS mbid FROM credits WHERE isrc IS NOT NULL AND mbid IS NOT NULL
	GROUP BY isrc HAVING count(DISTINCT mbid) = 1
), nameMbids AS (
	SELECT name, min(mbid) AS mbid FROM credits WHERE mbid IS NOT NULL GROUP BY name HAVING count(DISTINCT mbid) = 1
), nameIsrcs AS (
	SELECT name, min(isrc) AS isrc FROM credits WHERE isrc IS NOT NULL GROUP BY name HAVING count(DISTINCT isrc) = 1
), artistMbids AS (
	SELECT artistKey, min(artistMbid) AS mbid FROM credits WHERE artistMbid IS NOT NULL
	GROUP BY artistKey HAVING count(DISTINCT artistMbid) = 1
), plays AS MATERIALIZED (
	SELECT c.setUri, c.position, c.did, c.act, c.date, c.city, c.country, c.startSeconds, c.artistKey, c.artistName,
		c.track,
		coalesce(
			'mbid:' || coalesce(c.mbid, i.mbid, CASE WHEN c.isrc IS NULL OR ni.isrc = c.isrc THEN n.mbid END),
			'isrc:' || coalesce(c.isrc, ni.isrc),
			'name:' || c.name
		) AS trackId,
		coalesce('mbid:' || coalesce(c.artistMbid, am.mbid), 'name:' || c.artistKey) AS artistId
	FROM credits AS c
	LEFT JOIN isrcMbids AS i ON i.isrc = c.isrc
	LEFT JOIN nameMbids AS n ON n.name = c.name
	LEFT JOIN nameIsrcs AS ni ON ni.name = c.name
	LEFT JOIN artistMbids AS am ON am.artistKey = c.artistKey
)]]

-- PLAYS' placeholders, then `...`.
local function playsArgs(...)
	local args = { PROJECT, ROOT_DID, SET }
	for _, arg in ipairs({ ... }) do args[#args + 1] = arg end
	return args
end

-- Continues PLAYS, once `source` (a CTE of plays rows) is defined, with `topArtists` (the artists in it by supports:
-- each act's distinct tracks of theirs, at most MAX_SUPPORTS_PER_DJ, summed; its placeholder is the limit) and
-- `artistNames` (each one's name as plays credit it most).
local function rankArtists(source)
	return ', perAct AS (SELECT artistId, act, min(count(DISTINCT trackId), ' .. MAX_SUPPORTS_PER_DJ .. ') AS n'
		.. ' FROM ' .. source .. ' GROUP BY artistId, act)'
		.. ', topArtists AS (SELECT artistId, sum(n) AS supports, count(*) AS djCount FROM perAct GROUP BY artistId'
		.. ' ORDER BY supports DESC, djCount DESC, artistId LIMIT ?)'
		.. ', artistNames AS (SELECT artistId, artistName FROM (SELECT artistId, artistName,'
		.. ' row_number() OVER (PARTITION BY artistId ORDER BY count(*) DESC, max(date) DESC, artistName) AS rank'
		.. ' FROM plays WHERE artistId IN (SELECT artistId FROM topArtists) GROUP BY artistId, artistName)'
		.. ' WHERE rank = 1)'
end

local function artistView(row)
	return { name = row.name, mbid = row.id:match('^mbid:(.+)$'), supports = row.supports, djCount = row.djCount }
end

-- A track (JSON, as a set has it) as a trackView, with its id and `supports`.
local function trackView(id, encoded, supports)
	local track = json.decode(encoded)
	local artists = {}
	for i, artist in ipairs(track.artists) do artists[i] = { name = artist.artistName, mbid = artist.artistMbId } end
	return {
		id = id,
		title = track.title,
		artist = track.artist,
		artists = toarray(artists),
		recordingMbId = id:match('^mbid:(.+)$'),
		isrc = id:match('^isrc:(.+)$') or track.isrc,
		supports = supports
	}
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
