-- Records by URI, in the order asked for, each once. Ones HappyView doesn't have are left out.

function handle()
	local uris = list(params.uris, 25)
	local byUri = {}
	local rows = db.raw('SELECT uri, cid, record FROM happyview_records WHERE uri IN (' .. marks(#uris) .. ')', uris)
	for _, row in ipairs(rows) do byUri[row.uri] = row end
	local out = {}
	for _, uri in ipairs(uris) do
		if byUri[uri] then
			out[#out + 1] = recordView(byUri[uri])
			byUri[uri] = nil
		end
	end
	return { records = toarray(out) }
end
