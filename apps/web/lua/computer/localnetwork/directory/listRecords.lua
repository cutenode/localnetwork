-- One account's records of one collection, by record key, last first, as com.atproto.repo.listRecords lists them. The
-- cursor is the last record key of the page before, and only comes back when there's another page.

function handle()
	local actor, collection, cursor = text(params.actor), text(params.collection), text(params.cursor)
	if not (actor and collection) then return { records = toarray({}) } end
	local limit = pageSize(params.limit, 50, 100)
	local args = { actor, collection }
	if cursor then args[#args + 1] = cursor end
	args[#args + 1] = limit + 1
	local rows = db.raw(
		'SELECT uri, cid, rkey, record FROM happyview_records WHERE did = ? AND collection = ?'
			.. (cursor and ' AND rkey < ?' or '')
			.. ' ORDER BY rkey DESC LIMIT ?',
		args
	)
	local out = {}
	for i = 1, math.min(#rows, limit) do out[i] = recordView(rows[i]) end
	return { records = toarray(out), cursor = #rows > limit and rows[limit].rkey or nil }
end
