-- How many accounts are going to and interested in one of the root's calendar events, and the asking account's RSVPs
-- to it, if any. Each account counts once, by its latest RSVP to the event: record keys are TIDs, so within one account
-- the greatest URI is the newest.

local json = require('internal.json')
local db = require('happyview.db')

local RSVP = 'community.lexicon.calendar.rsvp'

local SUBJECT = " AND record::jsonb #>> '{subject,uri}' = ?"

local GOING = 'community.lexicon.calendar.rsvp#going'

local INTERESTED = 'community.lexicon.calendar.rsvp#interested'

function handle(input)
	local subject = text(input.subject) or ''
	-- Anyone can RSVP, so the accounts that did aren't ones any root record names, and raw SQL over every RSVP takes
	-- the store too long. HappyView's db library finds an event's RSVPs by its index, a page at a time.
	local latest, cursor = {}, nil
	repeat
		local page = db.records(RSVP):where('subject.uri', '=', subject):limit(100):cursor(cursor):run()
		for _, rsvp in ipairs(page.records) do
			local kept = latest[rsvp.did]
			if not kept or rsvp.uri > kept.uri then latest[rsvp.did] = { uri = rsvp.uri, status = rsvp.record.status } end
		end
		cursor = page.cursor
	until cursor == nil
	local counts = { [GOING] = 0, [INTERESTED] = 0 }
	for _, rsvp in pairs(latest) do
		if counts[rsvp.status] then counts[rsvp.status] = counts[rsvp.status] + 1 end
	end
	local actor = text(input.actor)
	local rsvps = {}
	if actor then
		local mine = query(
			"SELECT uri, record::jsonb ->> 'status' AS status FROM happyview_records WHERE did = ? AND collection = ?"
				.. SUBJECT
				.. ' ORDER BY uri COLLATE "C" LIMIT 100',
			{ actor, RSVP, subject }
		)
		for i, row in ipairs(mine) do
			rsvps[i] = { uri = row.uri, status = row.status }
		end
	end
	return { going = counts[GOING], interested = counts[INTERESTED], rsvps = json.to_array(rsvps) }
end
