-- Runs before HappyView stores a computer.localnetwork.epk event. Jetstream sends every account's press kits, and anyone
-- can write one naming any project, so only press kits of one of the root's projects get in: the root's own, which it
-- holds for the project's managers, and legacy ones by an account that manages the project they name, until they move
-- to the root. That's all the site reads (it checks again, so a manager who leaves takes theirs off the site). They're
-- kept as written: the app checks each one against its lexicon.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local PROJECT = 'computer.localnetwork.project'

local function gate()
	if action == 'delete' then return true end
	if type(record.project) ~= 'string' then return nil end
	local sql = 'SELECT 1 AS found FROM happyview_records WHERE uri = ? AND collection = ? AND did = ?'
	local args = { record.project, PROJECT, ROOT_DID }
	if did ~= ROOT_DID then
		sql = sql .. " AND EXISTS (SELECT 1 FROM json_each(record, '$.managers') WHERE value = ?)"
		args[#args + 1] = did
	end
	return #db.raw(sql .. ' LIMIT 1', args) > 0 or nil
end

function handle()
	local ok, result = pcall(gate)
	if ok then return result end
	log('skipped ' .. uri .. ': ' .. tostring(result))
	return nil
end
