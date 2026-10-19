-- Upcoming shows, soonest first, or nearest first and then soonest. SQL narrows by project, follower, and a bounding
-- box around the place, and pages when it can; Lua measures exact distances, then sorts and pages the rest.

local SOONEST = " ORDER BY datetime(json_extract(s.record, '$.startsAt')), s.uri"

function handle()
	local where, args = { UPCOMING }, {}
	local function filter(clause, values)
		where[#where + 1] = ' AND ' .. clause
		for _, value in ipairs(values) do args[#args + 1] = value end
	end

	local project = text(params.project)
	if project then filter("json_extract(s.record, '$.project') = ?", { project }) end
	local follower = text(params.follower)
	if follower then
		filter(
			"json_extract(s.record, '$.project') IN"
				.. " (SELECT json_extract(record, '$.subject') FROM happyview_records WHERE collection = ? AND did = ?)",
			{ FOLLOW, follower }
		)
	end
	local near = point(params.latE6, params.lngE6)
	local radius = near and integer(params.radiusKm)
	if radius then filter(withinBox(near, radius, 's.record', '$.location')) end
	local byDistance = near and text(params.sort) == 'distance'

	local limit = pageSize(params.limit, 30, 100)
	local offset = tonumber(text(params.cursor) or '')
	offset = math.type(offset) == 'integer' and offset > 0 and offset or 0

	-- Without an exact radius or a distance sort, SQL's order is the answer, so it reads only this page and one more
	-- row to tell whether there's a next one.
	local paged = not (radius or byDistance)
	local order = SOONEST
	if paged then
		order = order .. ' LIMIT ? OFFSET ?'
		args[#args + 1] = limit + 1
		args[#args + 1] = offset
	end

	local items = {}
	for i, row in ipairs(showRows(table.concat(where), args, order)) do
		local km = near and distanceKm(near, row.latE6, row.lngE6)
		if not radius or km <= radius then items[#items + 1] = { row = row, km = km, i = i } end
	end

	-- SQL already sorted soonest first; ties keep that order.
	if byDistance then
		table.sort(items, function(a, b)
			if a.km ~= b.km then return a.km < b.km end
			return a.i < b.i
		end)
	end

	local first = paged and 0 or offset
	local page = {}
	for i = first + 1, math.min(#items, first + limit) do page[#page + 1] = showView(items[i].row, items[i].km) end
	return { shows = toarray(page), cursor = first + limit < #items and tostring(offset + limit) or nil }
end
