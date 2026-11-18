-- Runs before HappyView stores a community.lexicon.calendar.event event. Jetstream sends every account's calendar
-- events, from every calendar app, so only events by an account that manages one of the root's projects get in: the
-- site reads only its shows' events. The check is on the author, not on a show naming the event, since the event can
-- arrive before its show. They're kept as written: the announce form keeps what other calendar apps added, and the
-- app checks each one against its lexicon.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip. A
-- require that fails while the script loads would happen outside it, so the gate requires the SQL library itself.

local json = require('internal.json')
local log = require('internal.logging')

local PROJECT = 'computer.localnetwork.project'

local function gate(input)
	if input.action == 'delete' then return true end
	local sql = require('happyview.sql')
	local rows = sql.raw(
		'SELECT 1 AS found FROM happyview_records WHERE did = $1 AND collection = $2'
			.. " AND record::jsonb -> 'managers' @> jsonb_build_array($3::text) LIMIT 1",
		json.to_array({ ROOT_DID, PROJECT, input.did })
	)
	return #rows > 0 or nil
end

function handle(input)
	local ok, result = pcall(gate, input)
	if ok then return result end
	log.info('skipped ' .. input.uri .. ': ' .. tostring(result))
	return nil
end
