-- One project by URI, else by slug (the oldest wins if a slug repeats), in any status. Lua can't answer with an
-- XRPC error, so a miss is an empty object. With `counts`, it also counts what the project's pages stream in after
-- it, so they can hold that much room: its press kits, by the root or a current manager (the ones the epk script
-- keeps), once each by record key since a press kit keeps its key when it moves to the root, and its upcoming shows.

local EPK = 'computer.localnetwork.epk'

local function pressKits(project)
	local managers = project.record.managers or {}
	local args = { EPK, project.uri, ROOT_DID }
	for _, did in ipairs(managers) do args[#args + 1] = did end
	return db.raw(
		'SELECT count(DISTINCT rkey) AS n FROM happyview_records'
			.. " WHERE collection = ? AND json_extract(record, '$.project') = ? AND did IN (" .. marks(#args - 2) .. ')',
		args
	)[1].n
end

function handle()
	local uri, slug = text(params.uri), text(params.slug)
	local found
	if uri then
		found = projects(' AND uri = ?', { uri })[1]
	elseif slug then
		found = projects(
			" AND json_extract(record, '$.slug') = ?",
			{ slug },
			" ORDER BY json_extract(record, '$.createdAt'), uri LIMIT 1"
		)[1]
	end
	if not found or params.counts ~= true then return { project = found } end
	return {
		project = found,
		pressKits = pressKits(found),
		upcomingShows = showCount(UPCOMING .. " AND json_extract(s.record, '$.project') = ?", { found.uri }),
	}
end
