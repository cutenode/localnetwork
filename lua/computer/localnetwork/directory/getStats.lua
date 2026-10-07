-- Directory totals across every status, and the genres and cities of active projects, most common first.

local WHERE = ' WHERE collection = ? AND did = ?'
local ACTIVE = WHERE .. " AND json_extract(record, '$.status') = 'active'"

function handle()
	local base = { PROJECT, ROOT_DID }
	local stats = db.raw(
		'SELECT COUNT(*) AS total,'
			.. " COUNT(CASE WHEN json_extract(record, '$.status') = 'active' THEN 1 END) AS active,"
			.. " COUNT(CASE WHEN json_array_length(record, '$.managers') = 0 THEN 1 END) AS unclaimed,"
			.. " COUNT(DISTINCT json_extract(record, '$.profile.location.city') || '|'"
			.. " || upper(json_extract(record, '$.profile.location.country'))) AS cityCount"
			.. ' FROM happyview_records'
			.. WHERE,
		base
	)[1]
	stats.genres = db.raw(
		'SELECT genre.value AS value, COUNT(*) AS count'
			.. " FROM happyview_records, json_each(record, '$.profile.genres') AS genre"
			.. ACTIVE
			.. ' GROUP BY genre.value ORDER BY count DESC, lower(value), value',
		base
	)
	stats.cities = db.raw(
		"SELECT json_extract(record, '$.profile.location.city') AS city,"
			.. " upper(json_extract(record, '$.profile.location.country')) AS country, COUNT(*) AS count"
			.. ' FROM happyview_records'
			.. ACTIVE
			.. ' GROUP BY city, country ORDER BY count DESC, lower(city), city, country',
		base
	)
	return stats
end
