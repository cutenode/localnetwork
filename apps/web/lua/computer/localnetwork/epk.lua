-- Runs before HappyView stores a computer.localnetwork.epk event. Jetstream sends every account's press kits, and anyone
-- can write one naming any project, so only press kits by an account that manages the root project they name get in,
-- which is all the site reads (it checks again, so a manager who leaves takes theirs off the site). They're kept as
-- written: the app checks each one against its lexicon.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local PROJECT = 'computer.localnetwork.project'

local function gate()
	if action == 'delete' then return true end
	if type(record.project) ~= 'string' then return nil end
	local rows = db.raw(
		'SELECT 1 AS found FROM happyview_records WHERE uri = ? AND collection = ? AND did = ?'
			.. " AND EXISTS (SELECT 1 FROM json_each(record, '$.managers') WHERE value = ?) LIMIT 1",
		{ record.project, PROJECT, ROOT_DID, did }
	)
	return #rows > 0 or nil
end

function handle()
	local ok, result = pcall(gate)
	if ok then return result end
	log('skipped ' .. uri .. ': ' .. tostring(result))
	return nil
end
