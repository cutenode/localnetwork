-- Shows by URI, past ones too, in the order asked for, each once. Ones the directory doesn't list are left out.

function handle()
	local uris = list(params.uris, 25)
	local byUri = {}
	for _, row in ipairs(showRows(' AND s.uri IN (' .. marks(#uris) .. ')', uris, '')) do byUri[row.uri] = row end
	local out = {}
	for _, uri in ipairs(uris) do
		if byUri[uri] then
			out[#out + 1] = showView(byUri[uri])
			byUri[uri] = nil
		end
	end
	return { shows = toarray(out) }
end
