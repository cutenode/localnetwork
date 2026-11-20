-- Records by URI, in the order asked for, each once. One HappyView doesn't have is read from its account's PDS when
-- its collection is one the reads go through for (lib/pds.lua), and left out otherwise.

local json = require('internal.json')

function handle(input)
	local uris = input.uris
	local byUri = fetchThrough(uris)
	local out = {}
	for _, uri in ipairs(uris) do
		if byUri[uri] then
			out[#out + 1] = byUri[uri]
			byUri[uri] = nil
		end
	end
	return { records = json.to_array(out) }
end
