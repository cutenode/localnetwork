-- Rank active projects by how well they fit a show. Each factor scores 0..1 and the weighted sum makes the score;
-- notes say why in codes that clients turn into text (see computer.localnetwork.directory.defs#matchNote).

-- Fee tiers by rank, cheapest first.
local TIER = {}
for rank, tier in ipairs(TAXONOMY.feeTiers) do TIER[tier] = rank end

local function percent(fraction)
	return math.floor(fraction * 100 + 0.5)
end

-- How well `offered` covers the show's `count` wanted genres: the average, over those, of the best similarity to
-- any offered genre, and the offered genres that matched one closely (0.6 or more), in wanted order. `related`
-- maps a genre to the wanted genres it resembles ({ index, similarity, ... }), so only related pairs cost anything.
local function genreFit(count, related, offered)
	if count == 0 then return 1, {} end
	if #offered == 0 then return 0, {} end
	-- Per wanted genre, the best similarity so far and the first offered genre to reach it.
	local best, from = {}, {}
	for j = 1, #offered do
		local hits = related[offered[j]]
		if hits then
			for k = 1, #hits, 2 do
				local i, value = hits[k], hits[k + 1]
				if value > (best[i] or 0) then best[i], from[i] = value, offered[j] end
			end
		end
	end
	local total, matches, seen = 0, {}, {}
	for i = 1, count do
		local value = best[i] or 0
		total = total + value
		if value >= 0.6 and not seen[from[i]] then
			seen[from[i]] = true
			matches[#matches + 1] = from[i]
		end
	end
	return total / count, matches
end

-- Scores one profile against the show: each factor in 0..1, notes on why, whether it's in range, and the distance
-- in km when both locations are known.
local function score(show, profile)
	local reasons, warnings = {}, {}

	local genre, matches = genreFit(show.count, show.related, normalizeGenres(profile.genres, 12))
	if show.count > 0 then
		if #matches > 0 then
			reasons[#reasons + 1] = { code = 'plays', genres = table.move(matches, 1, math.min(3, #matches), 1, {}) }
		elseif genre > 0 then
			reasons[#reasons + 1] = { code = 'adjacentSound' }
		else
			warnings[#warnings + 1] = { code = 'noGenreOverlap' }
		end
	end

	local km, distance, inRange = nil, 1, true
	if show.near then
		local location = profile.location
		if not (location.latE6 and location.lngE6) then
			distance = 0.3
			warnings[#warnings + 1] = { code = 'locationUnknown' }
		else
			km = distanceKm(show.near, location.latE6, location.lngE6)
			if km <= show.radius then
				distance = 1 - 0.4 * (km / show.radius)
				reasons[#reasons + 1] = km < 1 and { code = 'local' } or { code = 'nearby', distanceM = meters(km) }
			elseif (profile.travel and profile.travel.radiusKm or 0) >= km then
				distance = 0.5
				reasons[#reasons + 1] = { code = 'travels', distanceM = meters(km) }
			else
				distance, inRange = 0, false
				warnings[#warnings + 1] = { code = 'outOfRange', distanceM = meters(km) }
			end
		end
	end

	local budget = 1
	if show.tier then
		local have = TIER[profile.fee and profile.fee.tier]
		if not have then
			budget = 0.6
		elseif have <= show.tier then
			budget = 1 - 0.1 * (show.tier - have)
			reasons[#reasons + 1] = { code = 'fitsBudget' }
		elseif have == show.tier + 1 then
			budget = 0.35
			warnings[#warnings + 1] = { code = 'aboveBudget' }
		else
			budget = 0
			warnings[#warnings + 1] = { code = 'wellAboveBudget' }
		end
	end

	local capacity = 1
	if show.capacity then
		local range = profile.capacity or {}
		if range.min == nil and range.max == nil then
			capacity = 0.6
		else
			local low, high = range.min or 0, range.max or math.huge
			if show.capacity >= low and show.capacity <= high then
				reasons[#reasons + 1] = { code = 'fitsRoom' }
			elseif show.capacity >= low / 2 and show.capacity <= high * 2 then
				capacity = 0.5
			else
				capacity = 0.2
				warnings[#warnings + 1] = { code = show.capacity > high and 'biggerRoom' or 'smallerRoom' }
			end
		end
	end

	local setType = 1
	if show.setType then
		local offered = profile.setTypes or {}
		if #offered == 0 then
			setType = 0.6
		elseif contains(offered, show.setType) then
			reasons[#reasons + 1] = { code = 'setType', setType = show.setType }
		else
			setType = 0.5
		end
	end

	local format = 1
	if #show.formats > 0 then
		local offered = profile.formats or {}
		local shared = {}
		for _, value in ipairs(show.formats) do
			if contains(offered, value) then shared[#shared + 1] = value end
		end
		if #offered == 0 then
			format = 0.6
		elseif #shared > 0 then
			reasons[#reasons + 1] = { code = 'formats', formats = shared }
		else
			format = 0.4
			warnings[#warnings + 1] = { code = 'differentSetup' }
		end
	end

	return {
		total = 0.4 * genre + 0.25 * distance + 0.15 * budget + 0.08 * capacity + 0.07 * setType + 0.05 * format,
		inRange = inRange,
		factors = {
			genre = genre,
			distance = distance,
			budget = budget,
			capacity = capacity,
			setType = setType,
			format = format,
		},
		reasons = reasons,
		warnings = warnings,
	}, km
end

function handle()
	local capacity = integer(params.capacity)
	local wanted = normalizeGenres(list(params.genres, 12), 12)
	local show = {
		near = point(params.latE6, params.lngE6),
		radius = math.max(1, integer(params.radiusKm) or 50),
		count = #wanted,
		related = {},
		tier = TIER[params.budgetTier],
		capacity = capacity and capacity > 0 and capacity or nil,
		setType = text(params.setType),
		formats = list(params.formats, 10),
	}
	for i, want in ipairs(wanted) do
		for genre, value in pairs(similarities(want)) do
			local related = show.related[genre] or {}
			related[#related + 1] = i
			related[#related + 1] = value
			show.related[genre] = related
		end
	end
	local includeOutOfRange = params.includeOutOfRange == true

	local ranked = {}
	for i, project in ipairs(projects(" AND json_extract(record, '$.status') = 'active'", {})) do
		local result, km = score(show, project.record.profile)
		if includeOutOfRange or result.inRange then
			project.distanceM = km and meters(km)
			ranked[#ranked + 1] = {
				score = percent(result.total),
				km = km or math.huge,
				i = i,
				project = project,
				result = result,
			}
		end
	end
	-- Best score first, then nearest (unknown distances last), then newest.
	table.sort(ranked, function(a, b)
		if a.score ~= b.score then return a.score > b.score end
		if a.km ~= b.km then return a.km < b.km end
		return a.i < b.i
	end)

	-- Only the page gets percentages and note lists.
	local matches = {}
	for i = 1, math.min(#ranked, pageSize(params.limit, 30, 50)) do
		local result, breakdown = ranked[i].result, {}
		for factor, value in pairs(result.factors) do breakdown[factor] = percent(value) end
		matches[i] = {
			project = ranked[i].project,
			score = ranked[i].score,
			inRange = result.inRange,
			breakdown = breakdown,
			reasons = toarray(result.reasons),
			warnings = toarray(result.warnings),
		}
	end
	return { matches = toarray(matches) }
end
