-- Runs before HappyView stores a computer.localnetwork.follow event. Jetstream sends every account's follows, so only
-- follows of one of the root's projects get in, keeping just the subject and when.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local PROJECT = 'computer.localnetwork.project'

local function gate()
	if action == 'delete' then return true end
	local subject = record.subject
	if not (type(subject) == 'string' and type(record.createdAt) == 'string') then return nil end
	local rows = db.raw(
		'SELECT 1 AS found FROM happyview_records WHERE uri = ? AND collection = ? AND did = ? LIMIT 1',
		{ subject, PROJECT, ROOT_DID }
	)
	if #rows == 0 then return nil end
	return { subject = subject, createdAt = record.createdAt }
end

function handle()
	local ok, result = pcall(gate)
	if ok then return result end
	log('skipped ' .. uri .. ': ' .. tostring(result))
	return nil
end
