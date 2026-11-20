-- One account's records of one collection, by record key, last first, as com.atproto.repo.listRecords lists them. The
-- cursor is the last record key of the page before, and only comes back when there's another page. When HappyView
-- has none of them and the collection is one the reads go through for, they're read from the account's PDS
-- (lib/pds.lua).

local json = require('internal.json')

function handle(input)
	local actor, collection, cursor = text(input.actor), text(input.collection), text(input.cursor)
	if not (actor and collection) then return { records = json.to_array({}) } end
	local limit = pageSize(input.limit, 50, 100)
	local args = { actor, collection }
	if cursor then args[#args + 1] = cursor end
	args[#args + 1] = limit + 1
	local rows = query(
		'SELECT uri, cid, rkey, record FROM happyview_records WHERE did = ? AND collection = ?'
			.. (cursor and ' AND rkey COLLATE "C" < ?' or '')
			.. ' ORDER BY rkey COLLATE "C" DESC LIMIT ?',
		args
	)
	if #rows == 0 then
		local records, following = listThrough(actor, collection, limit, cursor)
		if records then return { records = json.to_array(records), cursor = following } end
	end
	local out = {}
	for i = 1, math.min(#rows, limit) do
		out[i] = recordView(rows[i])
	end
	return { records = json.to_array(out), cursor = #rows > limit and rows[limit].rkey or nil }
end
