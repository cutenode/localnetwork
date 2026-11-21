-- The sets some accounts published, most recently played first, then by URI so ties have a stable order.

function handle(input)
	local actors = input.actors
	local args = table.move(actors, 1, #actors, 1, {})
	args[#args + 1] = pageSize(input.limit, 50, 100)
	return {
		sets = sets(
			' AND s.did IN (' .. marks(#actors) .. ')',
			args,
			' ORDER BY (s.record::jsonb ->> \'date\') COLLATE "C" DESC NULLS LAST, s.uri COLLATE "C" LIMIT ?'
		),
	}
end
