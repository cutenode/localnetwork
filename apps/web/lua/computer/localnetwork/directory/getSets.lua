-- Sets by URI, in the order asked for, each once. Ones the record script kept out are left out.

local json = require('internal.json')

function handle(input)
	local uris = input.uris
	local where, values = atUris(uris, 's.')
	local byUri = {}
	for _, set in ipairs(sets(' AND ' .. where, values, '')) do
		byUri[set.uri] = set
	end
	local out = {}
	for _, uri in ipairs(uris) do
		if byUri[uri] then
			out[#out + 1] = byUri[uri]
			byUri[uri] = nil
		end
	end
	return { sets = json.to_array(out) }
end
