-- One project by URI, else by slug (the oldest wins if a slug repeats), in any status. Lua can't answer with an
-- XRPC error, so a miss is an empty object.

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
	return { project = found }
end
