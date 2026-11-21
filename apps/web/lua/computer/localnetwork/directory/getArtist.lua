-- One artist's supports: each of their tracks DJs played, most supported first, with every play (newest first), and
-- the projects each of those DJs manages. Asked for by `mbid`, or by `name`, which finds the artist that name is
-- filed under (the name alone when some plays have no MBID for it, otherwise its MBID's).

local json = require('internal.json')

local MAX_PLAYS = 1000

-- Each DJ's active projects, by name.
local function managed(dids)
	local args = { PROJECT, ROOT_DID }
	table.move(dids, 1, #dids, 3, args)
	local rows = query(
		"SELECT m AS did, p.record::jsonb ->> 'slug' AS slug, p.record::jsonb #>> '{profile,name}' AS name"
			.. " FROM happyview_records AS p, jsonb_array_elements_text(p.record::jsonb -> 'managers') AS m"
			.. " WHERE p.collection = ? AND p.did = ? AND p.record::jsonb ->> 'status' = 'active'"
			.. ' AND m IN ('
			.. marks(#dids)
			.. ') ORDER BY lower((p.record::jsonb #>> \'{profile,name}\') COLLATE "C"),'
			.. ' (p.record::jsonb ->> \'slug\') COLLATE "C"',
		args
	)
	local byDid = {}
	for _, row in ipairs(rows) do
		byDid[row.did] = byDid[row.did] or {}
		table.insert(byDid[row.did], { slug = row.slug, name = row.name })
	end
	local out = {}
	for i, did in ipairs(dids) do
		out[i] = { did = did, projects = json.to_array(byDid[did] or {}) }
	end
	return json.to_array(out)
end

function handle(input)
	skipInactive(input)
	local id = text(input.mbid) and mbid(input.mbid)
	local wanted = text(input.name)
	if not (id or wanted and not input.mbid) then return { tracks = json.to_array({}), djs = json.to_array({}) } end
	local find = id and 'SELECT ?::text AS artistId'
		or "SELECT artistId FROM plays WHERE artistKey = ? GROUP BY artistId ORDER BY artistId LIKE 'name:%' DESC,"
			.. ' count(*) DESC, artistId COLLATE "C" LIMIT 1'

	local rows = queryPlays(
		', wanted AS ('
			.. find
			.. ')'
			-- A track can credit an artist twice, so their plays are one row per set and position, under the name
			-- that sorts first.
			.. ', mine AS (SELECT DISTINCT ON (setUri, position) * FROM plays'
			.. ' WHERE artistId = (SELECT artistId FROM wanted) ORDER BY setUri, position, artistName COLLATE "C")'
			.. ', perAct AS (SELECT act, least(count(DISTINCT trackId), '
			.. MAX_SUPPORTS_PER_DJ
			.. ') AS n FROM mine GROUP BY act)'
			.. ', counts AS (SELECT trackId, count(DISTINCT act) AS supports, max(date COLLATE "C") AS last'
			.. ' FROM mine GROUP BY trackId)'
			.. ' SELECT (SELECT artistId FROM wanted) AS "artistId",'
			.. ' (SELECT artistName FROM mine GROUP BY artistName'
			.. ' ORDER BY count(*) DESC, max(date COLLATE "C") DESC, artistName COLLATE "C" LIMIT 1) AS name,'
			.. ' (SELECT sum(n) FROM perAct)::bigint AS "artistSupports", (SELECT count(*) FROM perAct) AS "djCount",'
			.. ' m.trackId AS id, c.supports, m.setUri AS uri, m.did, m.startSeconds AS "startSeconds",'
			-- The track as its latest play names it, once per track rather than on every row.
			.. ' CASE WHEN row_number() OVER (PARTITION BY m.trackId'
			.. ' ORDER BY m.date COLLATE "C" DESC, m.setUri COLLATE "C", m.position) = 1 THEN m.track::text END AS track'
			.. ' FROM mine AS m JOIN counts AS c ON c.trackId = m.trackId'
			.. ' ORDER BY c.supports DESC, c.last DESC, m.trackId COLLATE "C", m.date COLLATE "C" DESC,'
			.. ' m.setUri COLLATE "C", m.position LIMIT ?',
		{ id and 'mbid:' .. id or normalize(wanted), MAX_PLAYS }
	)
	if not rows[1] then return { tracks = json.to_array({}), djs = json.to_array({}) } end

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

	-- HappyView computes a record's uri when it's read, so a condition on uri reads every record. The sets are read
	-- by their DJs and rkeys instead, which an index finds, and any other set that pairing turns up is left out.
	local args = { SET }
	table.move(dids, 1, #dids, 2, args)
	for _, uri in ipairs(uris) do
		args[#args + 1] = uri:match('[^/]+$')
	end
	local setsByUri = {}
	for _, row in
		ipairs(
			query(
				'SELECT uri, cid, '
					.. SET_FIELDS
					.. ' FROM happyview_records WHERE collection = ? AND did IN ('
					.. marks(#dids)
					.. ') AND rkey IN ('
					.. marks(#uris)
					.. ')',
				args
			)
		)
	do
		if seen[row.uri] then setsByUri[row.uri] = setView(row.record, row.uri, row.cid) end
	end
	for i, track in ipairs(tracks) do
		for _, play in ipairs(track.plays) do
			play.set, play.uri = setsByUri[play.uri], nil
		end
		tracks[i] = { track = track.track, plays = json.to_array(track.plays) }
	end

	local first = rows[1]
	local artist = artistView({
		id = first.artistId,
		name = first.name,
		supports = first.artistSupports,
		djCount = first.djCount,
	})
	return { artist = artist, tracks = json.to_array(tracks), djs = managed(dids) }
end
