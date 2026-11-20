-- Runs before HappyView stores a computer.localnetwork.show event. Jetstream sends every account's shows, and anyone
-- can write one naming any project, so only shows by accounts that manage the project they name get in (the queries
-- check again, so a manager who leaves takes their shows off the listings). Only the fields the queries and listings
-- read are kept, each checked for its type, since HappyView stores whatever this returns.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip. A
-- require that fails while the script loads would happen outside it, so the gate requires the SQL library itself.

local json = require('internal.json')
local log = require('internal.logging')

local PROJECT = 'computer.localnetwork.project'

local function text(value)
	return type(value) == 'string' and value or nil
end

local function integer(value)
	return math.type(value) == 'integer' and value or nil
end

-- A time the directory's queries can read as an instant: an RFC 3339 date and time with its UTC offset, every field in
-- range, or nil. The record store's Postgres compares shows' times, and a time it can't read fails the whole query.
local function instant(value)
	local year, month, day, hour, minute, second, rest = (text(value) or ''):match(
		'^(%d%d%d%d)%-(%d%d)%-(%d%d)T(%d%d):(%d%d):(%d%d)(.*)$'
	)
	if not year then return nil end
	local zone = rest:gsub('^%.%d+', '', 1)
	local offsetHours, offsetMinutes = zone:match('^[+-](%d%d):(%d%d)$')
	if not (zone == 'Z' or offsetHours and tonumber(offsetHours) <= 15 and tonumber(offsetMinutes) <= 59) then
		return nil
	end
	-- Day 0 of the next month is the last of this one.
	local days = os.date('*t', os.time({ year = tonumber(year), month = tonumber(month) + 1, day = 0 })).day
	local fits = tonumber(month) >= 1 and tonumber(month) <= 12 and tonumber(day) >= 1 and tonumber(day) <= days
	return fits and tonumber(hour) <= 23 and tonumber(minute) <= 59 and tonumber(second) <= 59 and value or nil
end

-- Only http(s) links: listings make the ticket link clickable, and the lexicon's uri format also lets javascript: in.
local function webLink(value)
	local link = text(value)
	return link and link:lower():match('^https?://') and link or nil
end

-- Whether `account` manages the root's project at `project`, a URI. HappyView computes a record's uri when it's read,
-- so finding one by uri reads every record. It's found by the did, collection and rkey in its URI instead.
local function manages(account, project)
	local owner, collection, rkey = project:match('^at://([^/]+)/([^/]+)/([^/]+)$')
	if not (owner == ROOT_DID and collection == PROJECT) then return false end
	local sql = require('happyview.sql')
	local rows = sql.raw(
		'SELECT 1 AS found FROM happyview_records WHERE did = $1 AND collection = $2 AND rkey = $3'
			.. " AND record::jsonb -> 'managers' @> jsonb_build_array($4::text) LIMIT 1",
		json.to_array({ ROOT_DID, PROJECT, rkey, account })
	)
	return #rows > 0
end

-- The record as the directory keeps it, or nil with why it can't be one.
local function view(value)
	local event, place = value.event, value.location
	if not (type(event) == 'table' and text(event.uri) and text(event.cid)) then return nil, 'bad event' end
	if not (text(value.name) and instant(value.startsAt) and text(value.createdAt)) then
		return nil, 'bad name or times'
	end
	-- The coordinates found the venue's clock and place the show on a map, so a show without them can't be listed.
	if not (type(place) == 'table' and text(place.city) and text(place.country)) then return nil, 'bad place' end
	if not (integer(place.latE6) and integer(place.lngE6)) then return nil, 'bad place' end
	return {
		project = value.project,
		event = { uri = event.uri, cid = event.cid },
		name = value.name,
		startsAt = value.startsAt,
		endsAt = instant(value.endsAt),
		venue = text(value.venue),
		location = {
			city = place.city,
			region = text(place.region),
			country = place.country,
			latE6 = place.latE6,
			lngE6 = place.lngE6,
		},
		status = text(value.status),
		tickets = webLink(value.tickets),
		createdAt = value.createdAt,
	}
end

local function gate(input)
	if input.action == 'delete' then return true end
	if not (text(input.record.project) and manages(input.did, input.record.project)) then return nil end
	local out, why = view(input.record)
	if not out then log.info('skipped ' .. input.uri .. ': ' .. why) end
	return out
end

function handle(input)
	local ok, result = pcall(gate, input)
	if ok then return result end
	log.info('skipped ' .. input.uri .. ': ' .. tostring(result))
	return nil
end
