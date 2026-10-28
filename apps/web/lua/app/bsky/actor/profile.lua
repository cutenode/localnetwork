-- Runs before HappyView stores an app.bsky.actor.profile event. Jetstream sends every Bluesky profile change on the
-- network, and the site only reads the display names of accounts the root has verified or that manage one of its
-- projects (verification statuses compare them), so only those accounts' profiles get in, keeping just the display
-- name.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local VERIFICATION = 'app.bsky.graph.verification'
local PROJECT = 'computer.localnetwork.project'

local WANTED = 'SELECT 1 AS found FROM happyview_records WHERE did = ? AND ('
	.. "(collection = ? AND json_extract(record, '$.subject') = ?)"
	.. " OR (collection = ? AND EXISTS (SELECT 1 FROM json_each(record, '$.managers') WHERE value = ?))"
	.. ') LIMIT 1'

local function gate()
	if rkey ~= 'self' then return nil end
	if action == 'delete' then return true end
	if #db.raw(WANTED, { ROOT_DID, VERIFICATION, did, PROJECT, did }) == 0 then return nil end
	local name = record.displayName
	return { ['$type'] = collection, displayName = type(name) == 'string' and name or nil }
end

function handle()
	local ok, result = pcall(gate)
	if ok then return result end
	log('skipped ' .. uri .. ': ' .. tostring(result))
	return nil
end
