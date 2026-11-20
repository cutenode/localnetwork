-- Runs before HappyView stores a computer.localnetwork.follow event. Jetstream sends every account's follows, so only
-- follows of one of the root's projects or of a profile the directory keeps get in, keeping just the subject and when.
-- An account following its own profile is left out, so it can't count itself.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip. A
-- require that fails while the script loads would happen outside it, so the gate requires the SQL library itself.

local json = require('internal.json')
local log = require('internal.logging')

local PROJECT = 'computer.localnetwork.project'

local PROFILE = 'computer.localnetwork.profile'

local function gate(input)
	if input.action == 'delete' then return true end
	local subject = input.record.subject
	if not (type(subject) == 'string' and type(input.record.createdAt) == 'string') then return nil end
	if subject == 'at://' .. input.did .. '/' .. PROFILE .. '/self' then return nil end
	-- HappyView computes a record's uri when it's read, so finding one by uri reads every record. It's found by the
	-- did, collection and rkey in its URI instead.
	local owner, collection, rkey = subject:match('^at://([^/]+)/([^/]+)/([^/]+)$')
	if not (collection == PROJECT and owner == ROOT_DID or collection == PROFILE) then return nil end
	local sql = require('happyview.sql')
	local rows = sql.raw(
		'SELECT 1 AS found FROM happyview_records WHERE did = $1 AND collection = $2 AND rkey = $3 LIMIT 1',
		json.to_array({ owner, collection, rkey })
	)
	if #rows == 0 then return nil end
	return { subject = subject, createdAt = input.record.createdAt }
end

function handle(input)
	local ok, result = pcall(gate, input)
	if ok then return result end
	log.info('skipped ' .. input.uri .. ': ' .. tostring(result))
	return nil
end
