-- Runs before HappyView stores a computer.localnetwork.epk event. Jetstream sends every account's press kits, and anyone
-- can write one naming any project, so only press kits of one of the root's projects get in: the root's own, which it
-- holds for the project's managers, and legacy ones by an account that manages the project they name, until they move
-- to the root. That's all the site reads (it checks again, so a manager who leaves takes theirs off the site). They're
-- kept as written: the app checks each one against its lexicon.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip. A
-- require that fails while the script loads would happen outside it, so the gate requires the SQL library itself.

local json = require('internal.json')
local log = require('internal.logging')

local PROJECT = 'computer.localnetwork.project'

local function gate(input)
	if input.action == 'delete' then return true end
	if type(input.record.project) ~= 'string' then return nil end
	-- HappyView computes a record's uri when it's read, so finding one by uri reads every record. It's found by the
	-- did, collection and rkey in its URI instead.
	local owner, collection, rkey = input.record.project:match('^at://([^/]+)/([^/]+)/([^/]+)$')
	if not (owner == ROOT_DID and collection == PROJECT) then return nil end
	local query = 'SELECT 1 AS found FROM happyview_records WHERE did = $1 AND collection = $2 AND rkey = $3'
	local args = { ROOT_DID, PROJECT, rkey }
	if input.did ~= ROOT_DID then
		query = query .. " AND record::jsonb -> 'managers' @> jsonb_build_array($4::text)"
		args[#args + 1] = input.did
	end
	local sql = require('happyview.sql')
	return #sql.raw(query .. ' LIMIT 1', json.to_array(args)) > 0 or nil
end

function handle(input)
	local ok, result = pcall(gate, input)
	if ok then return result end
	log.info('skipped ' .. input.uri .. ': ' .. tostring(result))
	return nil
end
