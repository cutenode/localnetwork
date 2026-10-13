-- The sets some accounts published, most recently played first, then by URI so ties have a stable order.

function handle()
	local actors = list(params.actors, 10)
	local args = table.move(actors, 1, #actors, 1, {})
	args[#args + 1] = pageSize(params.limit, 50, 100)
	return {
		sets = sets(
			' AND did IN (' .. marks(#actors) .. ')',
			args,
			" ORDER BY json_extract(record, '$.date') DESC, uri LIMIT ?"
		)
	}
end
