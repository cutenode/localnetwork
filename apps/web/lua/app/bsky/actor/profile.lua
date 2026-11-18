-- Runs before HappyView stores an app.bsky.actor.profile event. Jetstream sends every Bluesky profile change on the
-- network, and the site only reads the display names and pronouns of accounts with a Local Network profile (whose
-- pages show their pronouns), that the root has verified, or that manage one of its projects (verification statuses
-- compare names), so only those accounts' profiles get in, keeping just the display name and pronouns.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip. A
-- require that fails while the script loads would happen outside it, so the gate requires the SQL library itself.

local json = require('internal.json')
local log = require('internal.logging')

local VERIFICATION = 'app.bsky.graph.verification'
local PROJECT = 'computer.localnetwork.project'
local PROFILE = 'computer.localnetwork.profile'

-- The root's verification of the account or a root project it manages, or the account's own profile.
local WANTED = 'SELECT 1 AS found FROM happyview_records WHERE (did = $1 AND ('
	.. "(collection = $2 AND record::jsonb ->> 'subject' = $3)"
	.. " OR (collection = $4 AND record::jsonb -> 'managers' @> jsonb_build_array($5::text))"
	.. ')) OR (did = $6 AND collection = $7) LIMIT 1'

local function text(value)
	return type(value) == 'string' and value or nil
end

local function gate(input)
	if input.rkey ~= 'self' then return nil end
	if input.action == 'delete' then return true end
	local sql = require('happyview.sql')
	if
		#sql.raw(WANTED, json.to_array({ ROOT_DID, VERIFICATION, input.did, PROJECT, input.did, input.did, PROFILE })) == 0
	then
		return nil
	end
	return {
		['$type'] = input.collection,
		displayName = text(input.record.displayName),
		pronouns = text(input.record.pronouns),
	}
end

function handle(input)
	local ok, result = pcall(gate, input)
	if ok then return result end
	log.info('skipped ' .. input.uri .. ': ' .. tostring(result))
	return nil
end
