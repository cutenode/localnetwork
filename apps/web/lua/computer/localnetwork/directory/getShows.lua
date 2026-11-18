-- Shows by URI, past ones too, in the order asked for, each once. Ones the directory doesn't list are left out.

local json = require('internal.json')

function handle(input)
	local uris = input.uris
	local where, values = atUris(uris, 's.')
	local byUri = {}
	for _, row in ipairs(showRows(' AND ' .. where, values, '')) do
		byUri[row.uri] = row
	end
	local out = {}
	for _, uri in ipairs(uris) do
		if byUri[uri] then
			out[#out + 1] = showView(byUri[uri])
			byUri[uri] = nil
		end
	end
	return { shows = json.to_array(out) }
end
