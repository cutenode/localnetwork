-- Search published projects. SQL applies the cheap filters (status, place, kind, fee tier, managers, a bounding
-- box, and a substring prefilter per search word); Lua applies genres, formats, set types, exact distance, and
-- word-prefix matching, then ranks and pages.

local BY_NAME = " ORDER BY lower(json_extract(record, '$.profile.name')), json_extract(record, '$.profile.name'), uri"

function handle()
	local where, args = {}, {}
	local function filter(clause, values)
		where[#where + 1] = ' AND ' .. clause
		for _, value in ipairs(values or {}) do args[#args + 1] = value end
	end

	local status = text(params.status)
	if status then
		filter("json_extract(record, '$.status') = ?", { status })
	elseif params.includeInactive ~= true then
		filter("json_extract(record, '$.status') = 'active'")
	end
	local country = text(params.country)
	if country then filter("upper(json_extract(record, '$.profile.location.country')) = upper(?)", { country }) end
	local kinds, tiers = list(params.kinds, 10), list(params.feeTiers, 10)
	if #kinds > 0 then filter("json_extract(record, '$.profile.kind') IN (" .. marks(#kinds) .. ')', kinds) end
	if #tiers > 0 then filter(inFeeTiers('record', '$.profile.fees', tiers, '$.profile.fee')) end
	if params.unclaimed == true then filter("json_array_length(record, '$.managers') = 0") end
	local manager = text(params.manager)
	if manager then filter("EXISTS (SELECT 1 FROM json_each(record, '$.managers') WHERE value = ?)", { manager }) end

	local needles = {}
	for _, term in ipairs(terms(text(params.q) or '')) do
		needles[#needles + 1] = ' ' .. term
		-- Every word appears somewhere in the record's JSON, a superset of the word-prefix match below.
		filter('record LIKE ?', { '%' .. term .. '%' })
	end

	local near = point(params.latE6, params.lngE6)
	local radius = integer(params.radiusKm) or 50
	if near then filter(withinBox(near, radius, 'record', '$.profile.location')) end

	local wanted = performerFilter(params)
	local sort = searchSort(params, needles, near)

	-- SQL sorts by name or newest first; searchPage keeps that order for ties.
	local items = {}
	for i, project in ipairs(projects(table.concat(where), args, sort == 'name' and BY_NAME or nil)) do
		local profile = project.record.profile
		if wanted(profile) then
			local km = near and distanceKm(near, profile.location.latE6, profile.location.lngE6)
			local rank = (not km or km <= radius) and (#needles == 0 or relevance(profile, needles))
			if rank then
				project.distanceM = km and meters(km)
				items[#items + 1] = { view = project, km = km, rank = rank, i = i }
			end
		end
	end

	local page, total, cursor = searchPage(items, sort, params, needles)
	return { projects = page, total = total, cursor = cursor }
end
