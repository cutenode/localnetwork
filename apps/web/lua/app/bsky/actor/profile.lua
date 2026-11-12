-- Runs before HappyView stores an app.bsky.actor.profile event. Jetstream sends every Bluesky profile change on the
-- network, and the site only reads the display names and pronouns of accounts with a Local Network profile (whose
-- pages show their pronouns), that the root has verified, or that manage one of its projects (verification statuses
-- compare names), so only those accounts' profiles get in, keeping just the display name and pronouns.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local VERIFICATION = 'app.bsky.graph.verification'
local PROJECT = 'computer.localnetwork.project'
local PROFILE = 'computer.localnetwork.profile'

-- The root's verification of the account or a root project it manages, or the account's own profile.
local WANTED = 'SELECT 1 AS found FROM happyview_records WHERE (did = ? AND ('
	.. "(collection = ? AND json_extract(record, '$.subject') = ?)"
	.. " OR (collection = ? AND EXISTS (SELECT 1 FROM json_each(record, '$.managers') WHERE value = ?))"
	.. ')) OR (did = ? AND collection = ?) LIMIT 1'

local function text(value)
	return type(value) == 'string' and value or nil
end

local function gate()
	if rkey ~= 'self' then return nil end
	if action == 'delete' then return true end
	if #db.raw(WANTED, { ROOT_DID, VERIFICATION, did, PROJECT, did, did, PROFILE }) == 0 then return nil end
	return { ['$type'] = collection, displayName = text(record.displayName), pronouns = text(record.pronouns) }
end

function handle()
	local ok, result = pcall(gate)
	if ok then return result end
	log('skipped ' .. uri .. ': ' .. tostring(result))
	return nil
end
