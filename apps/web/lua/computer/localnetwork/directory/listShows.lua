-- Upcoming shows, soonest first, or nearest first and then soonest. SQL narrows by project, follower, and a bounding
-- box around the place, and pages when it can; Lua measures exact distances, then sorts and pages the rest.

local json = require('internal.json')

local SOONEST = ' ORDER BY (s.record::jsonb ->> \'startsAt\')::timestamptz, s.uri COLLATE "C"'

function handle(input)
	skipInactive(input)
	local where, args = { UPCOMING }, {}
	local function filter(clause, values)
		where[#where + 1] = ' AND ' .. clause
		for _, value in ipairs(values) do
			args[#args + 1] = value
		end
	end

	local project = text(input.project)
	if project then filter("s.record::jsonb ->> 'project' = ?", { project }) end
	local follower = text(input.follower)
	if follower then
		filter(
			"s.record::jsonb ->> 'project' IN"
				.. " (SELECT record::jsonb ->> 'subject' FROM happyview_records WHERE did = ? AND collection = ?)",
			{ follower, FOLLOW }
		)
	end
	local near = point(input.latE6, input.lngE6)
	local radius = near and integer(input.radiusKm)
	if radius then filter(withinBox(near, radius, 's.record', 'location')) end
	local byDistance = near and text(input.sort) == 'distance'

	local limit = pageSize(input.limit, 30, 100)
	local offset = tonumber(text(input.cursor) or '')
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
	for i = first + 1, math.min(#items, first + limit) do
		page[#page + 1] = showView(items[i].row, items[i].km)
	end
	return { shows = json.to_array(page), cursor = first + limit < #items and tostring(offset + limit) or nil }
end
