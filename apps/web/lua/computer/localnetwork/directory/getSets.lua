-- Sets by URI, in the order asked for, each once. Ones the record script kept out are left out.

function handle()
	local uris = list(params.uris, 25)
	local byUri = {}
	for _, set in ipairs(sets(' AND uri IN (' .. marks(#uris) .. ')', uris, '')) do byUri[set.uri] = set end
	local out = {}
	for _, uri in ipairs(uris) do
		if byUri[uri] then
			out[#out + 1] = byUri[uri]
			byUri[uri] = nil
		end
	end
	return { sets = toarray(out) }
end
