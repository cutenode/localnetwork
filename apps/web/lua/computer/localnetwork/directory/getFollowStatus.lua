-- How many accounts follow a project or a profile, and the asking account's follows of it, if any. An account that
-- wrote two follows of one subject counts once, and unfollowing deletes both.

local json = require('internal.json')
local db = require('happyview.db')

local SUBJECT = " AND record::jsonb ->> 'subject' = ?"

function handle(input)
	local subject = text(input.subject) or ''
	-- Anyone can follow, so the followers aren't accounts any root record names, and raw SQL over every follow takes
	-- the store too long. HappyView's db library finds a subject's follows by its index, a page at a time, and each
	-- account counts once however many follows of it they wrote.
	local followers, seen, cursor = 0, {}, nil
	repeat
		local page = db.records(FOLLOW):where('subject', '=', subject):limit(100):cursor(cursor):run()
		for _, follow in ipairs(page.records) do
			if not seen[follow.did] then
				seen[follow.did] = true
				followers = followers + 1
			end
		end
		cursor = page.cursor
	until cursor == nil
	local actor = text(input.actor)
	local follows = {}
	if actor then
		local rows = query(
			'SELECT uri FROM happyview_records WHERE did = ? AND collection = ?'
				.. SUBJECT
				.. ' ORDER BY uri COLLATE "C" LIMIT 100',
			{ actor, FOLLOW, subject }
		)
		for i, row in ipairs(rows) do
			follows[i] = row.uri
		end
	end
	return { followers = followers, follows = json.to_array(follows) }
end
