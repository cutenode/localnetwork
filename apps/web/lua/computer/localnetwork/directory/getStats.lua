-- Directory totals across every status, and the genres and cities of active projects, most common first.

local WHERE = ' WHERE collection = ? AND did = ?'
local ACTIVE = WHERE .. " AND record::jsonb ->> 'status' = 'active'"

local CITY = "(record::jsonb #>> '{profile,location,city}')"
local COUNTRY = 'upper((record::jsonb #>> \'{profile,location,country}\') COLLATE "C")'

function handle()
	local base = { PROJECT, ROOT_DID }
	local stats = query(
		'SELECT count(*) AS total,'
			.. " count(CASE WHEN record::jsonb ->> 'status' = 'active' THEN 1 END) AS active,"
			.. " count(CASE WHEN record::jsonb -> 'managers' = '[]' THEN 1 END) AS unclaimed,"
			.. ' count(DISTINCT '
			.. CITY
			.. " || '|' || "
			.. COUNTRY
			.. ') AS "cityCount"'
			.. ' FROM happyview_records'
			.. WHERE,
		base
	)[1]
	stats.genres = query(
		'SELECT genre AS value, count(*) AS count'
			.. " FROM happyview_records, jsonb_array_elements_text(CASE WHEN left((record::jsonb #> '{profile,genres}')::text, 1)"
			.. " = '[' THEN record::jsonb #> '{profile,genres}' ELSE '[]' END) AS genre"
			.. ACTIVE
			.. ' GROUP BY genre ORDER BY count DESC, lower(genre COLLATE "C"), genre COLLATE "C"',
		base
	)
	stats.cities = query(
		'SELECT city, country, count(*) AS count FROM (SELECT '
			.. CITY
			.. ' AS city, '
			.. COUNTRY
			.. ' AS country FROM happyview_records'
			.. ACTIVE
			.. ') AS active GROUP BY city, country'
			.. ' ORDER BY count DESC, lower(city COLLATE "C"), city COLLATE "C", country COLLATE "C"',
		base
	)
	return stats
end
