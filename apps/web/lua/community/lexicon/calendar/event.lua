-- Runs before HappyView stores a community.lexicon.calendar.event event. Jetstream sends every account's calendar
-- events, from every calendar app, so only events by an account that manages one of the root's projects get in: the
-- site reads only its shows' events. The check is on the author, not on a show naming the event, since the event can
-- arrive before its show. They're kept as written: the announce form keeps what other calendar apps added, and the
-- app checks each one against its lexicon.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local PROJECT = 'computer.localnetwork.project'

local function gate()
	if action == 'delete' then return true end
	local rows = db.raw(
		'SELECT 1 AS found FROM happyview_records WHERE collection = ? AND did = ?'
			.. " AND EXISTS (SELECT 1 FROM json_each(record, '$.managers') WHERE value = ?) LIMIT 1",
		{ PROJECT, ROOT_DID, did }
	)
	return #rows > 0 or nil
end

function handle()
	local ok, result = pcall(gate)
	if ok then return result end
	log('skipped ' .. uri .. ': ' .. tostring(result))
	return nil
end
