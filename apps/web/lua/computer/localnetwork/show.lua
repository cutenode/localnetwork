-- Runs before HappyView stores a computer.localnetwork.show event. Jetstream sends every account's shows, and anyone
-- can write one naming any project, so only shows by accounts that manage the project they name get in (the queries
-- check again, so a manager who leaves takes their shows off the listings). Only the fields the queries and listings
-- read are kept, each checked for its type, since HappyView stores whatever this returns.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local PROJECT = 'computer.localnetwork.project'

local function text(value)
	return type(value) == 'string' and value or nil
end

local function integer(value)
	return math.type(value) == 'integer' and value or nil
end

-- Only http(s) links: listings make the ticket link clickable, and the lexicon's uri format also lets javascript: in.
local function webLink(value)
	local link = text(value)
	return link and link:lower():match('^https?://') and link or nil
end

local function manages(account, project)
	local rows = db.raw(
		'SELECT 1 AS found FROM happyview_records WHERE uri = ? AND collection = ? AND did = ?'
			.. " AND EXISTS (SELECT 1 FROM json_each(record, '$.managers') WHERE value = ?) LIMIT 1",
		{ project, PROJECT, ROOT_DID, account }
	)
	return #rows > 0
end

-- The record as the directory keeps it, or nil with why it can't be one.
local function view(value)
	local event, place = value.event, value.location
	if not (type(event) == 'table' and text(event.uri) and text(event.cid)) then return nil, 'bad event' end
	if not (text(value.name) and text(value.startsAt) and text(value.createdAt)) then return nil, 'bad name or times' end
	-- The coordinates found the venue's clock and place the show on a map, so a show without them can't be listed.
	if not (type(place) == 'table' and text(place.city) and text(place.country)) then return nil, 'bad place' end
	if not (integer(place.latE6) and integer(place.lngE6)) then return nil, 'bad place' end
	return {
		project = value.project,
		event = { uri = event.uri, cid = event.cid },
		name = value.name,
		startsAt = value.startsAt,
		endsAt = text(value.endsAt),
		venue = text(value.venue),
		location = {
			city = place.city,
			region = text(place.region),
			country = place.country,
			latE6 = place.latE6,
			lngE6 = place.lngE6
		},
		status = text(value.status),
		tickets = webLink(value.tickets),
		createdAt = value.createdAt
	}
end

local function gate()
	if action == 'delete' then return true end
	if not (text(record.project) and manages(did, record.project)) then return nil end
	local out, why = view(record)
	if not out then log('skipped ' .. uri .. ': ' .. why) end
	return out
end

function handle()
	local ok, result = pcall(gate)
	if ok then return result end
	log('skipped ' .. uri .. ': ' .. tostring(result))
	return nil
end
