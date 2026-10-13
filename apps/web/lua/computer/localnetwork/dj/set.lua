-- Runs before HappyView stores a computer.localnetwork.dj.set event. Jetstream sends every account's sets, but a
-- set only shows up on this site when a project's mix links it, and only managers can link one, so only sets by
-- accounts that manage one of the root's projects get in. Of those, only what the site shows is kept: the tracks
-- stay in the DJ's repo. HappyView keeps the event's CID either way.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local PROJECT = 'computer.localnetwork.project'

local ADDRESS = 'community.lexicon.location.address'

local function text(value)
	return type(value) == 'string' and value or nil
end

local function manages(account)
	local rows = db.raw(
		"SELECT 1 AS found FROM happyview_records WHERE collection = ? AND did = ?"
			.. " AND EXISTS (SELECT 1 FROM json_each(record, '$.managers') WHERE value = ?) LIMIT 1",
		{ PROJECT, ROOT_DID, account }
	)
	return #rows > 0
end

-- The directory's setView of the record, minus its URI and CID, or nil with why it can't be one.
local function view(value)
	if not (text(value.date) and type(value.tracks) == 'table') then return nil, 'bad date or tracks' end
	local out = {
		date = value.date,
		title = text(value.title),
		kind = text(value.kind),
		venue = text(value.venue),
		trackCount = #value.tracks
	}
	local place = value.location
	if type(place) == 'table' and place['$type'] == ADDRESS and text(place.country) then
		out.location = {
			country = place.country,
			name = text(place.name),
			locality = text(place.locality),
			region = text(place.region)
		}
	end
	return out
end

local function gate()
	if action == 'delete' then return true end
	if not manages(did) then return nil end
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
