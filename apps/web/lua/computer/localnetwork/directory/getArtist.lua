-- One artist's supports: each of their tracks DJs played, most supported first, with every play (newest first), and
-- the projects each of those DJs manages. Asked for by `mbid`, or by `name`, which finds the artist that name is
-- filed under (the name alone when some plays have no MBID for it, otherwise its MBID's).

local MAX_PLAYS = 1000

-- Each DJ's active projects, by name.
local function managed(dids)
	local args = { PROJECT, ROOT_DID }
	table.move(dids, 1, #dids, 3, args)
	local rows = db.raw(
		"SELECT m.value AS did, json_extract(p.record, '$.slug') AS slug,"
			.. " json_extract(p.record, '$.profile.name') AS name"
			.. " FROM happyview_records AS p, json_each(p.record, '$.managers') AS m"
			.. " WHERE p.collection = ? AND p.did = ? AND json_extract(p.record, '$.status') = 'active'"
			.. ' AND m.value IN ('
			.. marks(#dids)
			.. ') ORDER BY lower(name), slug',
		args
	)
	local byDid = {}
	for _, row in ipairs(rows) do
		byDid[row.did] = byDid[row.did] or {}
		table.insert(byDid[row.did], { slug = row.slug, name = row.name })
	end
	local out = {}
	for i, did in ipairs(dids) do out[i] = { did = did, projects = toarray(byDid[did] or {}) } end
	return toarray(out)
end

function handle()
	local id = text(params.mbid) and mbid(params.mbid)
	local wanted = text(params.name)
	if not (id or wanted and not params.mbid) then return { tracks = toarray({}), djs = toarray({}) } end
	local find = id and 'SELECT ? AS artistId'
		or "SELECT artistId FROM plays WHERE artistKey = ? GROUP BY artistId ORDER BY artistId LIKE 'name:%' DESC,"
			.. ' count(*) DESC, artistId LIMIT 1'

	local rows = db.raw(
		PLAYS
			.. ', wanted AS ('
			.. find
			.. ')'
			-- A track can credit an artist twice, so their plays are one row per set and position.
			.. ', mine AS (SELECT * FROM plays WHERE artistId = (SELECT artistId FROM wanted) GROUP BY setUri, position)'
			.. ', perAct AS (SELECT act, min(count(DISTINCT trackId), '
			.. MAX_SUPPORTS_PER_DJ
			.. ') AS n FROM mine GROUP BY act)'
			.. ', counts AS (SELECT trackId, count(DISTINCT act) AS supports, max(date) AS last, setUri, position'
			.. ' FROM mine GROUP BY trackId)'
			.. ' SELECT (SELECT artistId FROM wanted) AS artistId,'
			.. ' (SELECT artistName FROM mine GROUP BY artistName ORDER BY count(*) DESC, max(date) DESC, artistName'
			.. ' LIMIT 1) AS name,'
			.. ' (SELECT sum(n) FROM perAct) AS artistSupports, (SELECT count(*) FROM perAct) AS djCount,'
			.. ' m.trackId AS id, c.supports, m.setUri AS uri, m.did, m.startSeconds,'
			-- The track as its latest play names it, read once per track rather than on every row.
			.. ' CASE WHEN row_number() OVER (PARTITION BY m.trackId ORDER BY m.date DESC, m.setUri, m.position) = 1'
			.. ' THEN '
			.. playedTrack('ls', 'c')
			.. ' END AS track'
			.. ' FROM mine AS m JOIN counts AS c ON c.trackId = m.trackId'
			.. ' JOIN happyview_records AS ls ON ls.uri = c.setUri'
			.. ' ORDER BY c.supports DESC, c.last DESC, m.trackId, m.date DESC, m.setUri, m.position LIMIT ?',
		playsArgs(id and 'mbid:' .. id or normalize(wanted), MAX_PLAYS)
	)
	if not rows[1] then return { tracks = toarray({}), djs = toarray({}) } end

	-- Rows come grouped by track. Sets and DJs repeat, so each is read once, after.
	local tracks, byTrack, uris, dids, seen = {}, {}, {}, {}, {}
	for _, row in ipairs(rows) do
		local current = byTrack[row.id]
		if not current then
			current = { supports = row.supports, plays = {} }
			byTrack[row.id] = current
			tracks[#tracks + 1] = current
		end
		if row.track then current.track = trackView(row.id, row.track, row.supports) end
		current.plays[#current.plays + 1] = { did = row.did, uri = row.uri, startSeconds = row.startSeconds }
		if not seen[row.uri] then
			seen[row.uri] = true
			uris[#uris + 1] = row.uri
		end
		if not seen[row.did] then
			seen[row.did] = true
			dids[#dids + 1] = row.did
		end
	end

	local setsByUri = {}
	for _, row in ipairs(db.raw(
		'SELECT uri, cid, ' .. SET_FIELDS .. ' FROM happyview_records WHERE uri IN (' .. marks(#uris) .. ')',
		uris
	)) do
		setsByUri[row.uri] = setView(row.record, row.uri, row.cid)
	end
	for i, track in ipairs(tracks) do
		for _, play in ipairs(track.plays) do play.set, play.uri = setsByUri[play.uri], nil end
		tracks[i] = { track = track.track, plays = toarray(track.plays) }
	end

	local first = rows[1]
	local artist = artistView({
		id = first.artistId,
		name = first.name,
		supports = first.artistSupports,
		djCount = first.djCount
	})
	return { artist = artist, tracks = toarray(tracks), djs = managed(dids) }
end
