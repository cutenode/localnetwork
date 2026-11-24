-- The most supported tracks and artists in sets played between two days, optionally in one city, and the cities
-- the current DJs have sets from. A track charts once MIN_DJS_TO_CHART DJs support it, and an artist's supports only
-- count tracks that chart. Tracks and artists come from one statement, so the plays are built once.

local json = require('internal.json')

local function day(value)
	value = text(value)
	return value and value:match('^%d%d%d%d%-%d%d%-%d%d$')
end

function handle(input)
	skipInactive(input)
	local where, args = '', {}
	local since, before = day(input.since), day(input.before)
	local city, country = text(input.city), text(input.country)
	if since then
		where = where .. ' AND date COLLATE "C" >= ?'
		args[#args + 1] = since
	end
	if before then
		where = where .. ' AND date COLLATE "C" < ?'
		args[#args + 1] = before
	end
	if city and country then
		where = where
			.. ' AND lower(city COLLATE "C") = lower(?::text COLLATE "C") AND country = upper(?::text COLLATE "C")'
		args[#args + 1] = city
		args[#args + 1] = country
	end
	local limit = pageSize(input.limit, 50, 100)
	args[#args + 1] = limit
	args[#args + 1] = limit

	local rows, djs = queryPlays(
		', played AS (SELECT * FROM plays WHERE 1 = 1'
			.. where
			.. ')'
			.. ', charted AS (SELECT trackId, count(DISTINCT act) AS supports, max(date COLLATE "C") AS last FROM played'
			.. ' GROUP BY trackId HAVING count(DISTINCT act) >= '
			.. MIN_DJS_TO_CHART
			.. ')'
			.. ', topTracks AS (SELECT * FROM charted ORDER BY supports DESC, last DESC, trackId COLLATE "C" LIMIT ?)'
			-- The track as its latest play names it.
			.. ', latestTracks AS (SELECT DISTINCT ON (trackId) trackId, track FROM plays'
			.. ' WHERE trackId IN (SELECT trackId FROM topTracks) ORDER BY trackId, date COLLATE "C" DESC,'
			.. ' setUri COLLATE "C", position)'
			.. ', supported AS (SELECT * FROM played WHERE trackId IN (SELECT trackId FROM charted))'
			.. rankArtists('supported')
			.. ' SELECT \'track\' AS kind, t.trackId COLLATE "C" AS id, t.supports, t.last, NULL::bigint AS "djCount",'
			.. ' NULL AS name, l.track::text AS track FROM topTracks AS t JOIN latestTracks AS l ON l.trackId = t.trackId'
			.. ' UNION ALL SELECT \'artist\', a.artistId COLLATE "C", a.supports, NULL, a.djCount, n.artistName, NULL'
			.. ' FROM topArtists AS a JOIN artistNames AS n ON n.artistId = a.artistId'
			.. ' ORDER BY kind DESC, supports DESC, last DESC NULLS LAST, "djCount" DESC NULLS LAST, id',
		args
	)

	-- The cities the current DJs have sets with named tracks from (a first one, `-> 0`, only a list has).
	local places = json.to_array({})
	if djs[1] then
		places = query(
			'SELECT min(city COLLATE "C") AS city, country, count(*) AS count FROM (SELECT'
				.. " record::jsonb #>> '{location,locality}' AS city,"
				.. ' upper((record::jsonb #>> \'{location,country}\') COLLATE "C") AS country'
				.. ' FROM happyview_records WHERE collection = ? AND did IN ('
				.. marks(#djs)
				.. ')'
				.. " AND record::jsonb -> 'namedTracks' -> 0 IS NOT NULL) AS s"
				.. ' WHERE city IS NOT NULL AND country IS NOT NULL'
				.. ' GROUP BY lower(city COLLATE "C"), country'
				.. ' ORDER BY count DESC, lower(city COLLATE "C"), country COLLATE "C" LIMIT 100',
			append({ SET }, djs)
		)
	end

	local tracks, artists = {}, {}
	for _, row in ipairs(rows) do
		if row.kind == 'track' then
			tracks[#tracks + 1] = trackView(row.id, row.track, row.supports)
		else
			artists[#artists + 1] = artistView(row)
		end
	end
	return { tracks = json.to_array(tracks), artists = json.to_array(artists), places = places }
end
