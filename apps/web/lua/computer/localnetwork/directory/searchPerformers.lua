-- Search performers: active published projects, and DJs whose profile has the performer section on and that the root
-- lists (a computer.localnetwork.listing.profile keyed by their DID). Both are filtered and ranked the way
-- searchProjects does projects. A DJ's profile counts as a solo act, and when its performer section names no genres,
-- it's searched by the genres the DJ plays most, from their computer.localnetwork.dj.profile. A listed DJ who also
-- manages a solo project alone shows once, as their profile.

local PROFILE = 'computer.localnetwork.profile'

local LISTING = 'computer.localnetwork.listing.profile'

local DJ_PROFILE = 'computer.localnetwork.dj.profile'

local NAME = "json_extract(p.record, '$.basics.name')"

-- The DIDs of the DJs listed (a root listing, and a profile with the performer section on), for SQL. Its placeholders
-- are the profile and listing collections and the root's DID.
local LISTED_DJS = 'SELECT l.rkey FROM happyview_records AS l'
	.. " JOIN happyview_records AS p ON p.did = l.rkey AND p.collection = ? AND p.rkey = 'self'"
	.. " WHERE l.collection = ? AND l.did = ? AND json_type(p.record, '$.performer') = 'object'"

local PROFILES = "SELECT p.uri, p.cid, p.record, l.uri AS listing, json_extract(l.record, '$.createdAt') AS listedAt,"
	.. " CASE WHEN json_type(d.record, '$.genres') = 'array' THEN json_extract(d.record, '$.genres') END AS played"
	.. ' FROM happyview_records AS l'
	.. " JOIN happyview_records AS p ON p.did = l.rkey AND p.collection = ? AND p.rkey = 'self'"
	.. " LEFT JOIN happyview_records AS d ON d.did = l.rkey AND d.collection = ? AND d.rkey = 'self'"
	.. " WHERE l.collection = ? AND l.did = ? AND json_type(p.record, '$.performer') = 'object'"

local PROJECTS_BY_NAME = " ORDER BY lower(json_extract(record, '$.profile.name')),"
	.. " json_extract(record, '$.profile.name'), uri"

local PROFILES_BY_NAME = ' ORDER BY lower(' .. NAME .. '), ' .. NAME .. ', p.uri'

-- Newest listed first, as projects are newest published first.
local PROFILES_RECENT = " ORDER BY json_extract(l.record, '$.createdAt') DESC, p.uri"

-- The genres a DJ's profile is searched by: its performer section's, else up to 12 the DJ plays most (`played`, the
-- shares in their DJ stats as JSON, which their record script keeps as written).
local function profileGenres(performer, played)
	if performer.genres and #performer.genres > 0 then return performer.genres end
	local out = {}
	for _, share in ipairs(played and json.decode(played) or {}) do
		if type(share) == 'table' and type(share.name) == 'string' then out[#out + 1] = share.name end
		if #out == 12 then break end
	end
	return out
end

-- Whether search result `a` comes before `b` by name or newest first (`sort`), as SQL ordered each kind.
local function before(a, b, sort)
	if sort == 'name' then
		local x, y = a.name:lower(), b.name:lower()
		if x ~= y then return x < y end
		if a.name ~= b.name then return a.name < b.name end
	elseif a.at ~= b.at then
		return a.at > b.at
	end
	return a.uri < b.uri
end

function handle()
	local projectWhere, projectArgs = { " AND json_extract(record, '$.status') = 'active'" }, {}
	local profileWhere, profileArgs = {}, {}
	local function filter(where, args, clause, values)
		where[#where + 1] = ' AND ' .. clause
		for _, value in ipairs(values or {}) do args[#args + 1] = value end
	end
	local function project(clause, values) filter(projectWhere, projectArgs, clause, values) end
	local function profile(clause, values) filter(profileWhere, profileArgs, clause, values) end

	local country = text(params.country)
	if country then
		project("upper(json_extract(record, '$.profile.location.country')) = upper(?)", { country })
		profile("upper(json_extract(p.record, '$.basics.location.country')) = upper(?)", { country })
	end
	local kinds, tiers = list(params.kinds, 10), list(params.feeTiers, 10)
	if #kinds > 0 then project("json_extract(record, '$.profile.kind') IN (" .. marks(#kinds) .. ')', kinds) end
	local profiles = (#kinds == 0 or contains(kinds, 'solo')) and params.unclaimed ~= true
	if params.unclaimed == true then project("json_array_length(record, '$.managers') = 0") end
	-- A solo project its one manager stands behind as a listed DJ is them, so it gives way to their profile.
	if profiles then
		project("NOT (json_extract(record, '$.profile.kind') = 'solo' AND json_array_length(record, '$.managers') = 1"
			.. " AND json_extract(record, '$.managers[0]') IN (" .. LISTED_DJS .. '))', { PROFILE, LISTING, ROOT_DID })
	end
	if #tiers > 0 then
		project(inFeeTiers('record', '$.profile.fees', tiers, '$.profile.fee'))
		profile(inFeeTiers('p.record', '$.performer.fees', tiers))
	end

	local needles = {}
	for _, term in ipairs(terms(text(params.q) or '')) do
		needles[#needles + 1] = ' ' .. term
		-- Every word appears somewhere in the record's JSON (or a DJ's played genres), a superset of the word-prefix
		-- match below.
		local like = '%' .. term .. '%'
		project('record LIKE ?', { like })
		profile("(p.record LIKE ? OR json_extract(d.record, '$.genres') LIKE ?)", { like, like })
	end

	local near = point(params.latE6, params.lngE6)
	local radius = integer(params.radiusKm) or 50
	if near then
		project(withinBox(near, radius, 'record', '$.profile.location'))
		profile(withinBox(near, radius, 'p.record', '$.basics.location'))
	end

	local wanted = performerFilter(params)
	local sort = searchSort(params, needles, near)
	local order = sort == 'name' and 'name' or 'recent'

	-- Each kind's results that pass every filter, in SQL's order by name or newest first, with what merging them reads.
	local function keep(out, view, performer, at)
		if not wanted(performer) then return end
		local location = performer.location
		local km = near and distanceKm(near, location.latE6, location.lngE6)
		local rank = (not km or km <= radius) and (#needles == 0 or relevance(performer, needles))
		if not rank then return end
		view.distanceM = km and meters(km)
		out[#out + 1] = { view = view, km = km, rank = rank, name = performer.name, at = at, uri = view.uri }
	end

	local fromProjects = {}
	for _, row in ipairs(projects(table.concat(projectWhere), projectArgs, order == 'name' and PROJECTS_BY_NAME or nil)) do
		keep(fromProjects, row, row.record.profile, row.record.createdAt)
	end

	local fromProfiles = {}
	if profiles then
		local values = { PROFILE, DJ_PROFILE, LISTING, ROOT_DID }
		table.move(profileArgs, 1, #profileArgs, #values + 1, values)
		local rows = db.raw(
			PROFILES .. table.concat(profileWhere) .. (order == 'name' and PROFILES_BY_NAME or PROFILES_RECENT),
			values
		)
		for _, row in ipairs(rows) do
			local record = json.decode(row.record)
			local basics, performer = record.basics, record.performer
			local genres = profileGenres(performer, row.played)
			local view = { uri = row.uri, cid = row.cid, record = record, listing = row.listing, genres = toarray(genres) }
			keep(fromProfiles, view, {
				name = basics.name,
				tagline = performer.tagline,
				bio = basics.bio,
				lookingFor = performer.lookingFor,
				highlights = performer.highlights,
				genres = genres,
				formats = performer.formats,
				setTypes = performer.setTypes,
				location = basics.location,
			}, row.listedAt)
		end
	end

	-- Both lists are in order already, so merging them keeps it, and `i` records it for searchPage's ties.
	local items, a, b = {}, 1, 1
	while a <= #fromProjects or b <= #fromProfiles do
		local x, y = fromProjects[a], fromProfiles[b]
		local item
		if y == nil or (x ~= nil and before(x, y, order)) then
			item, a = x, a + 1
			item.view = { project = item.view }
		else
			item, b = y, b + 1
			item.view = { profile = item.view }
		end
		item.i = #items + 1
		items[item.i] = item
	end

	local page, total, cursor = searchPage(items, sort, params, needles)
	return { performers = page, total = total, cursor = cursor }
end
