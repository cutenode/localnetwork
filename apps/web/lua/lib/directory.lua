-- Shared by every directory script. The bundle (src/lib/server/happyview/bundle.ts) puts this after the prelude
-- that defines ROOT_DID and TAXONOMY and before the script itself, all in one chunk, so these locals are in scope
-- there, with every file's requires moved to the top. HappyView runs each request in a fresh VM, and runs top-level
-- code at upload too, with stand-ins for every module, so top-level code only builds lookups. Loading and handle()
-- each get HappyView's 1,000,000-instruction budget, which is why SQL narrows the rows before Lua touches them.

local json = require('internal.json')
local sql = require('happyview.sql')

local PROJECT = 'computer.localnetwork.project'

local SET = 'computer.localnetwork.dj.set'

local SHOW = 'computer.localnetwork.show'

local FOLLOW = 'computer.localnetwork.follow'

-- The mean Earth radius the app's distances have always used.
local EARTH_RADIUS_KM = 6371.0088

-- Newest first, then by URI so ties have a stable order.
local RECENT = ' ORDER BY (record::jsonb ->> \'createdAt\') COLLATE "C" DESC, uri COLLATE "C"'

-- Params ----------------------------------------------------------------------------------------------------------

-- HappyView checks each param against the lexicon before a script sees it: an array param is a list even when it's
-- given once, and no longer than its maxLength, an integer an integer, and a missing one with a default the default.

local function contains(values, wanted)
	for _, value in ipairs(values) do
		if value == wanted then return true end
	end
	return false
end

-- A string param, or nil when it's missing or empty.
local function text(value)
	return type(value) == 'string' and value ~= '' and value or nil
end

-- An integer param, or nil when it's missing.
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

-- `column`'s JSON number at `path` (a Postgres path, like 'location,latE6'), or NULL when it's anything else. A JSON
-- value's type is told by the first character of its text, since HappyView's raw SQL may only call the functions on
-- its allowlist, and jsonb_typeof isn't one.
local function number(column, path)
	local value = column .. "::jsonb #> '{" .. path .. "}'"
	return "(CASE WHEN strpos('-0123456789', left((" .. value .. ')::text, 1)) > 0 THEN (' .. value .. ')::numeric END)'
end

-- An SQL condition keeping the latE6 and lngE6 under `location` (a Postgres path into `column`, like
-- 'profile,location') inside a boundingBox around `center`, and its placeholders' values. A box that crosses the
-- antimeridian takes longitudes on both sides.
local function withinBox(center, radiusKm, column, location)
	local box = boundingBox(center, radiusKm)
	local lat = number(column, location .. ',latE6')
	local lng = number(column, location .. ',lngE6')
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

-- SQL -------------------------------------------------------------------------------------------------------------

-- HappyView reads the records from its record store, which is Postgres, keeps each record as JSON text (so SQL reads it
-- through record::jsonb), and answers within its time limit only when a query names the accounts it reads: one that
-- narrows by collection alone, or by uri, which the store computes from did, collection and rkey when it's read, reads
-- every record. So every query here starts from the root's own records, or from accounts those name. Text is compared
-- and sorted COLLATE "C", byte by byte, the order Lua's own comparisons use, and lowered that way too (Postgres's
-- lower() would also lower non-ASCII letters, which Lua's doesn't).

-- Runs `statement` with `args`. Postgres numbers its placeholders, and the statements here are built from fragments
-- that each use ?, so this numbers them in order. No other ? appears in their SQL. Postgres also folds unquoted names
-- to lowercase, so a column Lua reads by a camelCase name is aliased in quotes.
local function query(statement, args)
	local n = 0
	local numbered = statement:gsub('%?', function()
		n = n + 1
		return '$' .. n
	end)
	return sql.raw(numbered, json.to_array(args))
end

-- `values` and then `more`, as one list of placeholder values.
local function append(values, more)
	return table.move(more, 1, #more, #values + 1, values)
end

-- Records ---------------------------------------------------------------------------------------------------------

-- An SQL condition keeping the records at `uris` (with columns named `prefix` .. 'did' and so on, '' or an alias and a
-- dot) and its placeholders' values. It matches each URI's did, collection and rkey, which the store finds by its
-- index, so a URI without all three matches nothing. It's an OR per URI, so it's for lists as short as the lexicons' 25.
local function atUris(uris, prefix)
	local either, values = {}, {}
	for _, uri in ipairs(uris) do
		local did, collection, rkey = uri:match('^at://([^/]+)/([^/]+)/([^/]+)$')
		if did then
			either[#either + 1] = '(' .. prefix .. 'did = ? AND ' .. prefix .. 'collection = ? AND ' .. prefix .. 'rkey = ?)'
			append(values, { did, collection, rkey })
		end
	end
	return '(' .. (either[1] and table.concat(either, ' OR ') or 'false') .. ')', values
end

-- The root account's project records, as projectViews ({ uri, cid, record }), narrowed by `where` (SQL starting
-- with ' AND ', its placeholders filled from `args`) and sorted by `order` (newest first by default).
local function projects(where, args, order)
	local rows = query(
		'SELECT uri, cid, record FROM happyview_records WHERE did = ? AND collection = ?' .. where .. (order or RECENT),
		append({ ROOT_DID, PROJECT }, args)
	)
	for _, row in ipairs(rows) do
		row.record = json.decode(row.record)
	end
	return rows
end

-- The accounts the app found inactive (deactivated, taken down, suspended or deleted), as a set. HappyView keeps a
-- deactivated account's records, so the queries that read accounts' records take an `inactive` param and leave theirs
-- out. Each request runs in a fresh VM, so this starts empty every time.
local inactive = {}

-- Reads a query's `inactive` param into `inactive`. Call it first in handle().
local function skipInactive(input)
	for _, did in ipairs(input.inactive or {}) do
		inactive[did] = true
	end
end

-- The accounts that manage one of the root's projects (an active one, with `active`), each once, without inactive
-- ones: the accounts whose records the directory reads beyond the root's own.
local function managers(active)
	local rows = query(
		"SELECT DISTINCT m AS did FROM happyview_records, jsonb_array_elements_text(record::jsonb -> 'managers') AS m"
			.. ' WHERE did = ? AND collection = ?'
			.. (active and " AND record::jsonb ->> 'status' = 'active'" or '')
			.. ' ORDER BY 1',
		{ ROOT_DID, PROJECT }
	)
	local out = {}
	for _, row in ipairs(rows) do
		if not inactive[row.did] then out[#out + 1] = row.did end
	end
	return out
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
-- its event, which only the DJ sets block reads, the named tracks the support queries count, or the sources and
-- createdAt only locallibrary reads (through getRecords).
local SET_FIELDS = "(record::jsonb - 'tracks' - 'playedWith' - 'links' - 'recording' - 'event' - 'namedTracks'"
	.. " - 'sources' - 'createdAt')::text AS record"

-- Sets as setViews: the record script (computer/localnetwork/dj/set.lua) stores each one's view fields, tracklist,
-- and named tracks. It keeps every account's, so only sets by accounts that manage one of the root's projects, in any
-- status, are read here. Narrowed (with `where` on a set aliased s) and sorted like projects(), with `order` required.
local function sets(where, args, order)
	local djs = managers(false)
	if #djs == 0 then return json.to_array({}) end
	local rows = query(
		'SELECT uri, cid, '
			.. SET_FIELDS
			.. ' FROM happyview_records AS s WHERE s.did IN ('
			.. marks(#djs)
			.. ') AND s.collection = ?'
			.. where
			.. order,
		append(append(djs, { SET }), args)
	)
	local out = {}
	for i, row in ipairs(rows) do
		out[i] = setView(row.record, row.uri, row.cid)
	end
	return json.to_array(out)
end

-- Shows -------------------------------------------------------------------------------------------------------------

-- SQL (starting with ' AND ') for a show, aliased s, that hasn't ended. One without an end stays on for 12 hours after
-- it starts, through the night. The show record script keeps only times Postgres reads, with their UTC offsets, so
-- this compares instants.
local UPCOMING = " AND coalesce((s.record::jsonb ->> 'endsAt')::timestamptz,"
	.. " (s.record::jsonb ->> 'startsAt')::timestamptz + interval '12 hours') > now()"

-- What every one of the root's project URIs starts with.
local ROOT_PROJECTS = 'at://' .. ROOT_DID .. '/' .. PROJECT .. '/'

-- The FROM and WHERE of showRows and showCount, for shows by `djs`, and its placeholders' values. A show names its
-- project by URI, so the join takes the project's rkey from the end of it and checks the rest is the root's.
local function showsFrom(djs)
	return ' FROM happyview_records s JOIN happyview_records p ON p.did = ? AND p.collection = ?'
		.. " AND p.rkey = substr(s.record::jsonb ->> 'project', length(?) + 1)"
		.. " AND s.record::jsonb ->> 'project' = ? || p.rkey"
		.. ' WHERE s.did IN ('
		.. marks(#djs)
		.. ") AND s.collection = ? AND p.record::jsonb ->> 'status' = 'active'"
		.. " AND p.record::jsonb -> 'managers' @> jsonb_build_array(s.did)",
		append(append({ ROOT_DID, PROJECT, ROOT_PROJECTS, ROOT_PROJECTS }, djs), { SHOW })
end

-- Shows (aliased s) by a current manager of the active root project they name (aliased p), narrowed by `where` (SQL
-- starting with ' AND ') and followed by `order` (ORDER BY and LIMIT, or ''), their placeholders filled from `args`.
-- Each row has the show's uri, cid, and record JSON, its latE6 and lngE6, and the project's projectUri, projectCid,
-- and project JSON, so callers decode only the rows they return.
local function showRows(where, args, order)
	local djs = managers(true)
	if #djs == 0 then return {} end
	local from, values = showsFrom(djs)
	return query(
		'SELECT s.uri, s.cid, s.record,'
			.. ' (s.record::jsonb #>> \'{location,latE6}\')::bigint AS "latE6",'
			.. ' (s.record::jsonb #>> \'{location,lngE6}\')::bigint AS "lngE6",'
			.. ' p.uri AS "projectUri", p.cid AS "projectCid", p.record AS project'
			.. from
			.. where
			.. order,
		append(values, args)
	)
end

-- How many shows showRows would return for `where` and `args`, without reading them.
local function showCount(where, args)
	local djs = managers(true)
	if #djs == 0 then return 0 end
	local from, values = showsFrom(djs)
	return query('SELECT count(*) AS n' .. from .. where, append(values, args))[1].n
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

local UUID = '^'
	.. string.rep('%x', 8)
	.. string.rep('%-' .. string.rep('%x', 4), 3)
	.. '%-'
	.. string.rep('%x', 12)
	.. '$'

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
-- Its placeholders are the root DID and the project collection.
local DJS = [[djs AS (
	SELECT m AS did, min(p.uri COLLATE "C") AS act
	FROM happyview_records AS p, jsonb_array_elements_text(p.record::jsonb -> 'managers') AS m
	WHERE p.did = ? AND p.collection = ? AND p.record::jsonb ->> 'status' = 'active'
	GROUP BY m
)]]

-- What follows the sets CTE in queryPlays(): every named track of the current DJs' sets as a play, a row per play
-- and artist it credits, with the track and the artist it counts for. A track is its recording MBID, else its ISRC,
-- else its credit and title; an artist is their MBID, else their name. A collaboration ("A & B", "B x A") counts for
-- each of its acts when one of them is credited alone somewhere and nothing gives the whole credit an MBID, so
-- "Chase & Status" stays one act unless a set credits "Chase" or "Status" alone. A play without an ID takes the one other
-- plays have when only one fits (an ISRC's recording, a title's recording or ISRC, a name's artist), so sets
-- published before their writer filled in IDs count with the ones after. It reads every set each time, so each query
-- builds it once: Postgres materializes a CTE used more than once. Writing MATERIALIZED would fail, since HappyView
-- parses raw SQL with sqlparser's generic dialect and refuses what it can't parse as a write. Each play keeps its
-- track's JSON (`track`), since going back to a record for a track means Postgres parsing the whole record text,
-- tracklist and all, again: dozens of times slower over long sets. A named track or its artists that aren't a list count for nothing, where SQLite read whatever was there.
local PLAYS = [[, listed AS (
	SELECT s.uri AS setUri, t.position, s.did, s.act, s.date, s.city, s.country,
		(t.value ->> 'startSeconds')::bigint AS startSeconds, t.value ->> 'recordingMbId' AS mbid,
		t.value ->> 'isrc' AS isrc, t.value ->> 'key' AS name,
		a.value ->> 'artistMbId' AS artistMbid, a.value ->> 'key' AS artistKey,
		a.value ->> 'artistName' AS artistName, t.value AS track,
		CASE WHEN left((a.value -> 'parts')::text, 1) = '[' THEN a.value -> 'parts' END AS parts
	FROM sets AS s, jsonb_array_elements(s.named) WITH ORDINALITY AS t(value, position),
		jsonb_array_elements(CASE WHEN left((t.value -> 'artists')::text, 1) = '[' THEN t.value -> 'artists' END) AS a
), collabs AS (
	SELECT DISTINCT c.artistKey FROM listed AS c
	WHERE c.parts IS NOT NULL
		AND c.artistKey NOT IN (SELECT artistKey FROM listed WHERE artistMbid IS NOT NULL)
		AND EXISTS (SELECT 1 FROM jsonb_array_elements(c.parts) AS p
			WHERE p.value ->> 'key' IN (SELECT artistKey FROM listed WHERE parts IS NULL))
), credits AS (
	SELECT setUri, position, did, act, date, city, country, startSeconds, mbid, isrc, name, artistMbid, artistKey,
		artistName, track
	FROM listed WHERE artistKey NOT IN (SELECT artistKey FROM collabs)
	UNION ALL
	SELECT c.setUri, c.position, c.did, c.act, c.date, c.city, c.country, c.startSeconds, c.mbid, c.isrc, c.name,
		NULL, p.value ->> 'key', p.value ->> 'artistName', c.track
	FROM listed AS c, jsonb_array_elements(c.parts) AS p
	WHERE c.artistKey IN (SELECT artistKey FROM collabs)
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
), plays AS (
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

-- Runs the plays CTEs, then `statement` (more CTEs, then a SELECT) with `args` for its placeholders, and returns its
-- rows and the current DJs (managers(true)), with no rows when there are none. The sets are read by those DJs' DIDs,
-- which the store finds by its index. Each set's record is parsed once, in a subquery that OFFSET 0 keeps Postgres from
-- folding into the one that reads it, which would parse it again for every field.
local function queryPlays(statement, args)
	local djs = managers(true)
	if #djs == 0 then return json.to_array({}), djs end
	local rows = query(
		'WITH '
			.. DJS
			.. ', sets AS (SELECT uri, did, act,'
			.. " CASE WHEN left((r -> 'namedTracks')::text, 1) = '[' THEN r -> 'namedTracks' END AS named,"
			.. " r ->> 'date' AS date, r #>> '{location,locality}' AS city,"
			.. ' upper((r #>> \'{location,country}\') COLLATE "C") AS country'
			.. ' FROM (SELECT s.uri, s.did, d.act, s.record::jsonb AS r'
			.. ' FROM happyview_records AS s JOIN djs AS d ON d.did = s.did WHERE s.did IN ('
			.. marks(#djs)
			.. ') AND s.collection = ? OFFSET 0) AS parsed)'
			.. PLAYS
			.. statement,
		append(append({ ROOT_DID, PROJECT }, djs), append({ SET }, args))
	)
	return rows, djs
end

-- Continues queryPlays' CTEs, once `source` (a CTE of plays rows) is defined, with `topArtists` (the artists in it
-- by supports: each act's distinct tracks of theirs, at most MAX_SUPPORTS_PER_DJ, summed; its placeholder is the
-- limit) and `artistNames` (each one's name as plays credit it most).
local function rankArtists(source)
	return ', perAct AS (SELECT artistId, act, least(count(DISTINCT trackId), '
		.. MAX_SUPPORTS_PER_DJ
		.. ') AS n'
		.. ' FROM '
		.. source
		.. ' GROUP BY artistId, act)'
		.. ', topArtists AS (SELECT artistId, sum(n)::bigint AS supports, count(*) AS djCount FROM perAct'
		.. ' GROUP BY artistId ORDER BY supports DESC, djCount DESC, artistId COLLATE "C" LIMIT ?)'
		.. ', artistNames AS (SELECT artistId, artistName FROM (SELECT artistId, artistName, row_number() OVER'
		.. ' (PARTITION BY artistId ORDER BY count(*) DESC, max(date COLLATE "C") DESC, artistName COLLATE "C") AS rank'
		.. ' FROM plays WHERE artistId IN (SELECT artistId FROM topArtists) GROUP BY artistId, artistName) AS ranked'
		.. ' WHERE rank = 1)'
end

local function artistView(row)
	return { name = row.name, mbid = row.id:match('^mbid:(.+)$'), supports = row.supports, djCount = row.djCount }
end

-- A track (JSON, as a set has it) as a trackView, with its id and `supports`.
local function trackView(id, encoded, supports)
	local track = json.decode(encoded)
	local artists = {}
	for i, artist in ipairs(track.artists) do
		artists[i] = { name = artist.artistName, mbid = artist.artistMbId }
	end
	return {
		id = id,
		title = track.title,
		artist = track.artist,
		artists = json.to_array(artists),
		recordingMbId = id:match('^mbid:(.+)$'),
		isrc = id:match('^isrc:(.+)$') or track.isrc,
		supports = supports,
	}
end

-- Genres ----------------------------------------------------------------------------------------------------------

-- Family by genre, then by family id, so a family's id (like 'bass') finds it even when it isn't one of its genres.
local FAMILY = {}
for _, family in ipairs(TAXONOMY.families) do
	for _, genre in ipairs(family.genres) do
		FAMILY[genre] = family
	end
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

-- Performers ------------------------------------------------------------------------------------------------------

-- searchProjects and searchPerformers filter and rank a performer, a project's profile or a DJ's, by what both have:
-- name, tagline, bio, lookingFor, highlights, genres, formats, setTypes, and location (which a DJ can leave out).

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

local find, concat = string.find, table.concat

-- How well a performer matches `needles` (' ' .. term each), weighting fields like the old full-text index did
-- (name 10, tagline 4, about 1, genres 6, place 3) for every term found. nil when some term matches nowhere. It runs
-- for every performer a search's words prefilter keeps, so it's written out field by field, without tables.
local function relevance(performer, needles)
	local location = performer.location or {}
	local name, tagline = words(performer.name), words(performer.tagline or '')
	local about = words(
		(performer.bio or '') .. ' ' .. (performer.lookingFor or '') .. ' ' .. concat(performer.highlights or {}, ' ')
	)
	local genres = words(concat(performer.genres, ' '))
	local place = words((location.city or '') .. ' ' .. (location.region or '') .. ' ' .. (location.country or ''))
	local total = 0
	for i = 1, #needles do
		local needle = needles[i]
		local found = (find(name, needle, 1, true) and 10 or 0)
			+ (find(tagline, needle, 1, true) and 4 or 0)
			+ (find(about, needle, 1, true) and 1 or 0)
			+ (find(genres, needle, 1, true) and 6 or 0)
			+ (find(place, needle, 1, true) and 3 or 0)
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

-- An SQL condition keeping records with a fee level (the objects at `fees`, a Postgres path into `column`, like
-- 'profile,fees') in one of `tiers`, or whose one fee from before fee levels (at `legacy`, when given) is, and its
-- placeholders' values. Fees that aren't a list count as none, rather than failing the query.
local function inFeeTiers(column, fees, tiers, legacy)
	local inTiers, values = ' IN (' .. marks(#tiers) .. ')', table.move(tiers, 1, #tiers, 1, {})
	local list = column .. "::jsonb #> '{" .. fees .. "}'"
	local clause = 'EXISTS (SELECT 1 FROM jsonb_array_elements(CASE WHEN left(('
		.. list
		.. ")::text, 1) = '[' THEN "
		.. list
		.. " ELSE '[]' END) AS fee WHERE fee ->> 'tier'"
		.. inTiers
		.. ')'
	if not legacy then return clause, values end
	table.move(tiers, 1, #tiers, #tiers + 1, values)
	return '(' .. clause .. ' OR ' .. column .. "::jsonb #>> '{" .. legacy .. ",tier}'" .. inTiers .. ')', values
end

-- The filters SQL can't apply, short of distance and search words, from the genres, formats and setTypes params: a
-- function saying whether a performer plays one of those genres or a close subgenre, and has one of those formats
-- and set types, each only when the params ask for any.
local function performerFilter(params)
	local close = {}
	for _, want in ipairs(normalizeGenres(params.genres or {}, 20)) do
		for genre, value in pairs(similarities(want)) do
			if value >= 0.6 then close[genre] = true end
		end
	end
	local wantsGenres = next(close) ~= nil
	local formats, setTypes = params.formats or {}, params.setTypes or {}
	return function(performer)
		if not (overlaps(formats, performer.formats) and overlaps(setTypes, performer.setTypes)) then return false end
		if not wantsGenres then return true end
		for _, genre in ipairs(normalizeGenres(performer.genres, 12)) do
			if close[genre] then return true end
		end
		return false
	end
end

-- How a search sorts: as the sort param says, else by relevance with search words, by distance from a place, or
-- newest first.
local function searchSort(params, needles, near)
	return text(params.sort) or (#needles > 0 and 'relevance') or (near and 'distance') or 'recent'
end

-- Sorts search results ({ view, km, rank, i }, in order by name or newest first, which `i` numbers) by relevance (when
-- there were search words) or distance when `sort` asks, ties keeping that order, and returns a page of their views,
-- the total, and the next cursor.
local function searchPage(items, sort, params, needles)
	if sort == 'relevance' and #needles > 0 then
		-- Relevance is a small whole number, so grouping by it sorts in one pass, each group in order, where a
		-- comparison sort would cost thousands of instructions more per page.
		local groups, top = {}, 0
		for _, item in ipairs(items) do
			local group = groups[item.rank]
			if not group then
				group = {}
				groups[item.rank], top = group, math.max(top, item.rank)
			end
			group[#group + 1] = item
		end
		items = {}
		for rank = top, 1, -1 do
			for _, item in ipairs(groups[rank] or {}) do
				items[#items + 1] = item
			end
		end
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
	for i = offset + 1, math.min(#items, offset + limit) do
		page[#page + 1] = items[i].view
	end
	return json.to_array(page), #items, offset + limit < #items and tostring(offset + limit) or nil
end
