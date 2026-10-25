-- Runs before HappyView stores an app.bsky.graph.follow event. Jetstream sends every Bluesky follow on the network
-- (about 40 a second), and the site only reads the follows of press kit owners whose kit treats people they follow
-- differently from anyone signed in (the "following" audience rule), so only theirs get in, keeping just the subject
-- and when. It runs for every follow on the network, so it makes one indexed lookup and nothing else.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local EPK = 'computer.localnetwork.epk'

local function gate()
	if action == 'delete' then return true end
	if not (type(record.subject) == 'string' and type(record.createdAt) == 'string') then return nil end
	local rows = db.raw(
		'SELECT 1 AS found FROM happyview_records WHERE did = ? AND collection = ?'
			.. " AND json_extract(record, '$.access.following') IS NOT json_extract(record, '$.access.anyone') LIMIT 1",
		{ did, EPK }
	)
	if #rows == 0 then return nil end
	return { subject = record.subject, createdAt = record.createdAt }
end

function handle()
	local ok, result = pcall(gate)
	if ok then return result end
	log('skipped ' .. uri .. ': ' .. tostring(result))
	return nil
end
