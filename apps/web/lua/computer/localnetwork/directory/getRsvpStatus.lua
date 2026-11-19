-- How many accounts are going to and interested in one of the root's calendar events, and the asking account's RSVPs
-- to it, if any. Each account counts once, by its latest RSVP to the event: record keys are TIDs, so within one account
-- the greatest URI is the newest.

local RSVP = 'community.lexicon.calendar.rsvp'

local SUBJECT = " AND json_extract(record, '$.subject.uri') = ?"

local GOING = 'community.lexicon.calendar.rsvp#going'

local INTERESTED = 'community.lexicon.calendar.rsvp#interested'

function handle()
	local subject = text(params.subject) or ''
	local counts = { [GOING] = 0, [INTERESTED] = 0 }
	local rows = db.raw(
		"SELECT json_extract(record, '$.status') AS status, COUNT(*) AS count FROM happyview_records WHERE uri IN"
			.. ' (SELECT MAX(uri) FROM happyview_records WHERE collection = ?'
			.. SUBJECT
			.. ' GROUP BY did)'
			.. ' GROUP BY status',
		{ RSVP, subject }
	)
	for _, row in ipairs(rows) do
		counts[row.status] = row.count
	end
	local actor = text(params.actor)
	local rsvps = {}
	if actor then
		local mine = db.raw(
			"SELECT uri, json_extract(record, '$.status') AS status FROM happyview_records WHERE collection = ? AND did = ?"
				.. SUBJECT
				.. ' ORDER BY uri LIMIT 100',
			{ RSVP, actor, subject }
		)
		for i, row in ipairs(mine) do
			rsvps[i] = { uri = row.uri, status = row.status }
		end
	end
	return { going = counts[GOING], interested = counts[INTERESTED], rsvps = toarray(rsvps) }
end
