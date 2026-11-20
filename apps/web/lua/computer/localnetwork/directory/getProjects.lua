-- Projects by URI in any status, in the order asked for, each once. Unknown URIs are left out.

local json = require('internal.json')

function handle(input)
	local uris = input.uris
	local where, values = atUris(uris, '')
	local byUri = {}
	for _, project in ipairs(projects(' AND ' .. where, values)) do
		byUri[project.uri] = project
	end
	local out = {}
	for _, uri in ipairs(uris) do
		if byUri[uri] then
			out[#out + 1] = byUri[uri]
			byUri[uri] = nil
		end
	end
	return { projects = json.to_array(out) }
end
