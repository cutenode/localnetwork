-- Runs before HappyView stores an app.bsky.graph.follow event. Jetstream sends every Bluesky follow on the network
-- (about 40 a second), and the site only reads the follows of the managers of a project with a press kit that treats
-- people they follow differently from anyone signed in (the "following" audience rule, which any of its managers'
-- follows meet), so only theirs get in, keeping just the subject and when. It runs for every follow on the network, so
-- it starts from the few press kits that read follows rather than from the projects.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local EPK = 'computer.localnetwork.epk'
local PROJECT = 'computer.localnetwork.project'

local READER = 'SELECT 1 AS found FROM happyview_records kit'
	.. ' JOIN happyview_records project ON project.uri = json_extract(kit.record, \'$.project\')'
	.. ' WHERE kit.collection = ?'
	.. " AND json_extract(kit.record, '$.access.following') IS NOT json_extract(kit.record, '$.access.anyone')"
	.. ' AND project.collection = ? AND project.did = ?'
	.. " AND EXISTS (SELECT 1 FROM json_each(project.record, '$.managers') WHERE value = ?) LIMIT 1"

local function gate()
	if action == 'delete' then return true end
	if not (type(record.subject) == 'string' and type(record.createdAt) == 'string') then return nil end
	if #db.raw(READER, { EPK, PROJECT, ROOT_DID, did }) == 0 then return nil end
	return { subject = record.subject, createdAt = record.createdAt }
end

function handle()
	local ok, result = pcall(gate)
	if ok then return result end
	log('skipped ' .. uri .. ': ' .. tostring(result))
	return nil
end
