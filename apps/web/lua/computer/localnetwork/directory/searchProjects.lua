-- Search published projects. SQL applies the cheap filters (status, place, kind, fee tier, managers, a bounding
-- box, and a substring prefilter per search word); Lua applies genres, formats, set types, exact distance, and
-- word-prefix matching, then ranks and pages.

local NAME = "(record::jsonb #>> '{profile,name}')"

local BY_NAME = ' ORDER BY lower(' .. NAME .. ' COLLATE "C"), ' .. NAME .. ' COLLATE "C", uri COLLATE "C"'

function handle(input)
	local where, args = {}, {}
	local function filter(clause, values)
		where[#where + 1] = ' AND ' .. clause
		for _, value in ipairs(values or {}) do
			args[#args + 1] = value
		end
	end

	local status = text(input.status)
	if status then
		filter("record::jsonb ->> 'status' = ?", { status })
	elseif input.includeInactive ~= true then
		filter("record::jsonb ->> 'status' = 'active'")
	end
	local country = text(input.country)
	if country then
		filter(
			'upper((record::jsonb #>> \'{profile,location,country}\') COLLATE "C") = upper(?::text COLLATE "C")',
			{ country }
		)
	end
	local kinds, tiers = input.kinds or {}, input.feeTiers or {}
	if #kinds > 0 then filter("record::jsonb #>> '{profile,kind}' IN (" .. marks(#kinds) .. ')', kinds) end
	if #tiers > 0 then filter(inFeeTiers('record', 'profile,fees', tiers, 'profile,fee')) end
	if input.unclaimed == true then filter("record::jsonb -> 'managers' = '[]'") end
	local manager = text(input.manager)
	if manager then filter("record::jsonb -> 'managers' @> jsonb_build_array(?::text)", { manager }) end

	local needles = {}
	for _, term in ipairs(terms(text(input.q) or '')) do
		needles[#needles + 1] = ' ' .. term
		-- Every word appears somewhere in the record's JSON, a superset of the word-prefix match below. ILIKE under "C"
		-- ignores ASCII case only, as terms() lowers.
		filter('record COLLATE "C" ILIKE ?', { '%' .. term .. '%' })
	end

	local near = point(input.latE6, input.lngE6)
	local radius = integer(input.radiusKm) or 50
	if near then filter(withinBox(near, radius, 'record', 'profile,location')) end

	local wanted = performerFilter(input)
	local sort = searchSort(input, needles, near)

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

	local page, total, cursor = searchPage(items, sort, input, needles)
	return { projects = page, total = total, cursor = cursor }
end
