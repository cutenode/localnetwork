-- Artists whose name has the search text in it (every artist without one), most supported first.

function handle()
	local q = text(params.q)
	local limit = pageSize(params.limit, 25, 50)
	local where, args = '', playsArgs(limit)
	if q then
		where = ' WHERE instr(artistKey, ?) > 0'
		args = playsArgs(normalize(q), limit)
	end
	local rows = db.raw(
		PLAYS
			.. ', matched AS (SELECT * FROM plays WHERE artistId IN (SELECT artistId FROM plays'
			.. where
			.. '))'
			.. rankArtists('matched')
			.. ' SELECT a.artistId AS id, n.artistName AS name, a.supports, a.djCount FROM topArtists AS a'
			.. ' JOIN artistNames AS n ON n.artistId = a.artistId'
			.. ' ORDER BY a.supports DESC, a.djCount DESC, a.artistId',
		args
	)
	local out = {}
	for i, row in ipairs(rows) do out[i] = artistView(row) end
	return { artists = toarray(out) }
end
