-- Projects by URI in any status, in the order asked for, each once. Unknown URIs are left out.

function handle()
	local uris = list(params.uris, 25)
	local byUri = {}
	for _, project in ipairs(projects(' AND uri IN (' .. marks(#uris) .. ')', uris)) do byUri[project.uri] = project end
	local out = {}
	for _, uri in ipairs(uris) do
		if byUri[uri] then
			out[#out + 1] = byUri[uri]
			byUri[uri] = nil
		end
	end
	return { projects = toarray(out) }
end
