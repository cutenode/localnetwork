-- One project by URI, else by slug (the oldest wins if a slug repeats), in any status. Lua can't answer with an
-- XRPC error, so a miss is an empty object. With `counts`, it also counts what the project's pages stream in after
-- it, so they can hold that much room: its press kits, by the root or a current manager (the ones the epk script
-- keeps), once each by record key since a press kit keeps its key when it moves to the root, its upcoming shows,
-- and its managers with DJ stats or sets on file.

local EPK = 'computer.localnetwork.epk'

local DJ_PROFILE = 'computer.localnetwork.dj.profile'

local function pressKits(project)
	local managers = project.record.managers or {}
	local args = { EPK, project.uri, ROOT_DID }
	for _, did in ipairs(managers) do
		args[#args + 1] = did
	end
	return query(
		'SELECT count(DISTINCT rkey) AS n FROM happyview_records'
			.. " WHERE collection = ? AND record::jsonb ->> 'project' = ? AND did IN ("
			.. marks(#args - 2)
			.. ')',
		args
	)[1].n
end

local function djs(project)
	local managers = project.record.managers or {}
	if #managers == 0 then return 0 end
	local args = { DJ_PROFILE, SET }
	for _, did in ipairs(managers) do
		args[#args + 1] = did
	end
	return query(
		'SELECT count(DISTINCT did) AS n FROM happyview_records WHERE collection IN (?, ?) AND did IN ('
			.. marks(#managers)
			.. ')',
		args
	)[1].n
end

function handle(input)
	local uri, slug = text(input.uri), text(input.slug)
	local found
	if uri then
		local where, values = atUris({ uri }, '')
		found = projects(' AND ' .. where, values)[1]
	elseif slug then
		found = projects(
			" AND record::jsonb ->> 'slug' = ?",
			{ slug },
			' ORDER BY (record::jsonb ->> \'createdAt\') COLLATE "C", uri COLLATE "C" LIMIT 1'
		)[1]
	end
	if not found or input.counts ~= true then return { project = found } end
	return {
		project = found,
		pressKits = pressKits(found),
		upcomingShows = showCount(UPCOMING .. " AND s.record::jsonb ->> 'project' = ?", { found.uri }),
		djs = djs(found),
	}
end
