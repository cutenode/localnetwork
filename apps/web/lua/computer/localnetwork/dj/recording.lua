-- Runs before HappyView stores a computer.localnetwork.dj.recording event: a set's recording, which locallibrary
-- uploads to the DJ's own repo and the set's record points at. Jetstream sends every account's, and all of them are
-- kept, as sets are, since anyone can come to manage a project. What's kept is what the site needs to play one: the
-- audio blob as written, how long it plays, and createdAt, since locallibrary reads its recordings back through
-- getRecords before replacing one, and keeps the first createdAt. Anything else is dropped.
-- A recording without an audio blob isn't kept at all. HappyView keeps the event's CID either way.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local function blob(value)
	if type(value) ~= 'table' or value['$type'] ~= 'blob' or type(value.ref) ~= 'table' then return nil end
	local size = value.size
	if type(value.ref['$link']) ~= 'string' or type(value.mimeType) ~= 'string' then return nil end
	if math.type(size) ~= 'integer' or size < 0 then return nil end
	return value
end

local function gate()
	if action == 'delete' then return true end
	local audio = blob(record.audio)
	if not audio then
		log('skipped ' .. uri .. ': no audio blob')
		return nil
	end
	local seconds = record.durationSeconds
	return {
		audio = audio,
		durationSeconds = math.type(seconds) == 'integer' and seconds > 0 and seconds or nil,
		createdAt = type(record.createdAt) == 'string' and record.createdAt or nil
	}
end

function handle()
	local ok, result = pcall(gate)
	if ok then return result end
	log('skipped ' .. uri .. ': ' .. tostring(result))
	return nil
end
