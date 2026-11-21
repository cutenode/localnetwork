-- Search performers: active published projects, and DJs whose profile has the performer section on and that the root
-- lists (a computer.localnetwork.listing.profile keyed by their DID). Both are filtered and ranked the way
-- searchProjects does projects. A DJ's profile counts as a solo act, and when its performer section names no genres,
-- it's searched by the genres the DJ plays most, from their computer.localnetwork.dj.profile. A listed DJ who also
-- manages a solo project alone shows once, as their profile.

local json = require('internal.json')

local PROFILE = 'computer.localnetwork.profile'

local LISTING = 'computer.localnetwork.listing.profile'

local DJ_PROFILE = 'computer.localnetwork.dj.profile'

local NAME = "(p.record::jsonb #>> '{basics,name}')"

-- Listed DJs' profiles (aliased p) with the root's listing of each (l, keyed by the DJ's DID) and their DJ stats (d),
-- up to the IN list of the DJs' DIDs. Its placeholders are the root's DID and the listing, DJ stats and profile
-- collections. `played` is the DJ stats' genres as JSON.
local PROFILES = 'SELECT p.uri, p.cid, p.record, l.uri AS listing,'
	.. ' l.record::jsonb ->> \'createdAt\' AS "listedAt",'
	.. " CASE WHEN left((d.record::jsonb -> 'genres')::text, 1) = '[' THEN d.record::jsonb ->> 'genres' END AS played"
	.. ' FROM happyview_records AS p'
	.. ' JOIN happyview_records AS l ON l.did = ? AND l.collection = ? AND l.rkey = p.did'
	.. " LEFT JOIN happyview_records AS d ON d.did = p.did AND d.collection = ? AND d.rkey = 'self'"
	.. " WHERE p.collection = ? AND p.rkey = 'self' AND p.did IN ("

local PROJECTS_BY_NAME = ' ORDER BY lower((record::jsonb #>> \'{profile,name}\') COLLATE "C"),'
	.. ' (record::jsonb #>> \'{profile,name}\') COLLATE "C", uri COLLATE "C"'

local PROFILES_BY_NAME = ' ORDER BY lower(' .. NAME .. ' COLLATE "C"), ' .. NAME .. ' COLLATE "C", p.uri COLLATE "C"'

-- Newest listed first, as projects are newest published first.
local PROFILES_RECENT = ' ORDER BY (l.record::jsonb ->> \'createdAt\') COLLATE "C" DESC, p.uri COLLATE "C"'

-- The DIDs of the DJs listed: the root lists each by their DID as its key, and their profile has the performer
-- section on. Read first, so the queries after name the accounts they read.
local function listedDjs()
	local rows = query('SELECT rkey FROM happyview_records WHERE did = ? AND collection = ?', { ROOT_DID, LISTING })
	if #rows == 0 then return {} end
	local listed = {}
	for i, row in ipairs(rows) do
		listed[i] = row.rkey
	end
	rows = query(
		'SELECT did FROM happyview_records WHERE did IN ('
			.. marks(#listed)
			.. ") AND collection = ? AND rkey = 'self' AND left((record::jsonb -> 'performer')::text, 1) = '{'",
		append(listed, { PROFILE })
	)
	local out = {}
	for i, row in ipairs(rows) do
		out[i] = row.did
	end
	return out
end

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

function handle(input)
	local projectWhere, projectArgs = { " AND record::jsonb ->> 'status' = 'active'" }, {}
	local profileWhere, profileArgs = {}, {}
	local function filter(where, args, clause, values)
		where[#where + 1] = ' AND ' .. clause
		for _, value in ipairs(values or {}) do
			args[#args + 1] = value
		end
	end
	local function project(clause, values)
		filter(projectWhere, projectArgs, clause, values)
	end
	local function profile(clause, values)
		filter(profileWhere, profileArgs, clause, values)
	end

	local country = text(input.country)
	if country then
		project(
			'upper((record::jsonb #>> \'{profile,location,country}\') COLLATE "C") = upper(?::text COLLATE "C")',
			{ country }
		)
		profile(
			'upper((p.record::jsonb #>> \'{basics,location,country}\') COLLATE "C") = upper(?::text COLLATE "C")',
			{ country }
		)
	end
	local kinds, tiers = input.kinds or {}, input.feeTiers or {}
	if #kinds > 0 then project("record::jsonb #>> '{profile,kind}' IN (" .. marks(#kinds) .. ')', kinds) end
	local profiles = (#kinds == 0 or contains(kinds, 'solo')) and input.unclaimed ~= true
	if input.unclaimed == true then project("record::jsonb -> 'managers' = '[]'") end
	local djs = profiles and listedDjs() or {}
	-- A solo project its one manager stands behind as a listed DJ is them, so it gives way to their profile.
	if #djs > 0 then
		project(
			"NOT (record::jsonb #>> '{profile,kind}' = 'solo' AND jsonb_array_length(record::jsonb -> 'managers') = 1"
				.. " AND record::jsonb #>> '{managers,0}' IN ("
				.. marks(#djs)
				.. '))',
			djs
		)
	end
	if #tiers > 0 then
		project(inFeeTiers('record', 'profile,fees', tiers, 'profile,fee'))
		profile(inFeeTiers('p.record', 'performer,fees', tiers))
	end

	local needles = {}
	for _, term in ipairs(terms(text(input.q) or '')) do
		needles[#needles + 1] = ' ' .. term
		-- Every word appears somewhere in the record's JSON (or a DJ's played genres), a superset of the word-prefix
		-- match below. ILIKE under "C" ignores ASCII case only, as terms() lowers.
		local like = '%' .. term .. '%'
		project('record COLLATE "C" ILIKE ?', { like })
		profile('(p.record COLLATE "C" ILIKE ? OR (d.record::jsonb ->> \'genres\') COLLATE "C" ILIKE ?)', { like, like })
	end

	local near = point(input.latE6, input.lngE6)
	local radius = integer(input.radiusKm) or 50
	if near then
		project(withinBox(near, radius, 'record', 'profile,location'))
		profile(withinBox(near, radius, 'p.record', 'basics,location'))
	end

	local wanted = performerFilter(input)
	local sort = searchSort(input, needles, near)
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
	if #djs > 0 then
		local rows = query(
			PROFILES
				.. marks(#djs)
				.. ')'
				.. table.concat(profileWhere)
				.. (order == 'name' and PROFILES_BY_NAME or PROFILES_RECENT),
			append(append({ ROOT_DID, LISTING, DJ_PROFILE, PROFILE }, djs), profileArgs)
		)
		for _, row in ipairs(rows) do
			local record = json.decode(row.record)
			local basics, performer = record.basics, record.performer
			local genres = profileGenres(performer, row.played)
			local view =
				{ uri = row.uri, cid = row.cid, record = record, listing = row.listing, genres = json.to_array(genres) }
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

	local page, total, cursor = searchPage(items, sort, input, needles)
	return { performers = page, total = total, cursor = cursor }
end
