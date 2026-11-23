-- Artists whose name has the search text in it (every artist without one), most supported first.

local json = require('internal.json')

function handle(input)
	skipInactive(input)
	local q = text(input.q)
	local limit = pageSize(input.limit, 25, 50)
	local where, args = '', { limit }
	if q then
		where = ' WHERE strpos(artistKey, ?) > 0'
		args = { normalize(q), limit }
	end
	local rows = queryPlays(
		', matched AS (SELECT * FROM plays WHERE artistId IN (SELECT artistId FROM plays'
			.. where
			.. '))'
			.. rankArtists('matched')
			.. ' SELECT a.artistId AS id, n.artistName AS name, a.supports, a.djCount AS "djCount" FROM topArtists AS a'
			.. ' JOIN artistNames AS n ON n.artistId = a.artistId'
			.. ' ORDER BY a.supports DESC, a.djCount DESC, a.artistId COLLATE "C"',
		args
	)
	local out = {}
	for i, row in ipairs(rows) do
		out[i] = artistView(row)
	end
	return { artists = json.to_array(out) }
end
