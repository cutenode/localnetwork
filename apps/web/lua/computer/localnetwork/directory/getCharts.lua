-- The most supported tracks and artists in sets played between two days, optionally in one city, and the cities
-- the current DJs have sets from. A track charts once MIN_DJS_TO_CHART DJs support it, and an artist's supports only
-- count tracks that chart. Tracks and artists come from one statement, so PLAYS is built once.

local function day(value)
	value = text(value)
	return value and value:match('^%d%d%d%d%-%d%d%-%d%d$')
end

function handle()
	local where, args = '', {}
	local since, before = day(params.since), day(params.before)
	local city, country = text(params.city), text(params.country)
	if since then
		where = where .. ' AND date >= ?'
		args[#args + 1] = since
	end
	if before then
		where = where .. ' AND date < ?'
		args[#args + 1] = before
	end
	if city and country then
		where = where .. ' AND lower(city) = lower(?) AND country = upper(?)'
		args[#args + 1] = city
		args[#args + 1] = country
	end
	local limit = pageSize(params.limit, 50, 100)
	args[#args + 1] = limit
	args[#args + 1] = limit
	args = playsArgs(table.unpack(args))

	local rows = db.raw(
		PLAYS
			.. ', played AS (SELECT * FROM plays WHERE 1 = 1'
			.. where
			.. ')'
			.. ', charted AS (SELECT trackId, count(DISTINCT act) AS supports, max(date) AS last FROM played'
			.. ' GROUP BY trackId HAVING count(DISTINCT act) >= '
			.. MIN_DJS_TO_CHART
			.. ')'
			.. ', topTracks AS (SELECT * FROM charted ORDER BY supports DESC, last DESC, trackId LIMIT ?)'
			.. ', latestTracks AS (SELECT trackId, max(date) AS latest, setUri, position FROM plays'
			.. ' WHERE trackId IN (SELECT trackId FROM topTracks) GROUP BY trackId)'
			.. ', supported AS (SELECT * FROM played WHERE trackId IN (SELECT trackId FROM charted))'
			.. rankArtists('supported')
			.. " SELECT 'track' AS kind, t.trackId AS id, t.supports, t.last, NULL AS djCount, NULL AS name, "
			.. playedTrack('s', 'l')
			.. ' AS track FROM topTracks AS t JOIN latestTracks AS l ON l.trackId = t.trackId'
			.. ' JOIN happyview_records AS s ON s.uri = l.setUri'
			.. " UNION ALL SELECT 'artist', a.artistId, a.supports, NULL, a.djCount, n.artistName, NULL"
			.. ' FROM topArtists AS a JOIN artistNames AS n ON n.artistId = a.artistId'
			.. ' ORDER BY kind DESC, supports DESC, last DESC, djCount DESC, id',
		args
	)

	local places = db.raw(
		'WITH '
			.. DJS
			.. " SELECT json_extract(record, '$.location.locality') AS city,"
			.. " upper(json_extract(record, '$.location.country')) AS country, COUNT(*) AS count"
			.. ' FROM happyview_records WHERE collection = ? AND did IN (SELECT did FROM djs)'
			.. " AND json_extract(record, '$.location.locality') IS NOT NULL"
			.. " AND json_extract(record, '$.location.country') IS NOT NULL"
			.. " AND json_array_length(record, '$.tracks') > 0"
			.. ' GROUP BY lower(city), country ORDER BY count DESC, lower(city), country LIMIT 100',
		playsArgs()
	)

	local tracks, artists = {}, {}
	for _, row in ipairs(rows) do
		if row.kind == 'track' then
			tracks[#tracks + 1] = trackView(row.id, row.track, row.supports)
		else
			artists[#artists + 1] = artistView(row)
		end
	end
	return { tracks = toarray(tracks), artists = toarray(artists), places = places }
end
