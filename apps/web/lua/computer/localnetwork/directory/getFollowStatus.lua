-- How many accounts follow a project or a profile, and the asking account's follows of it, if any. An account that
-- wrote two follows of one subject counts once, and unfollowing deletes both.

local SUBJECT = " AND json_extract(record, '$.subject') = ?"

function handle()
	local subject = text(params.subject) or ''
	local count = db.raw(
		'SELECT COUNT(DISTINCT did) AS followers FROM happyview_records WHERE collection = ?' .. SUBJECT,
		{ FOLLOW, subject }
	)[1]
	local actor = text(params.actor)
	local follows = {}
	if actor then
		local rows = db.raw(
			'SELECT uri FROM happyview_records WHERE collection = ? AND did = ?' .. SUBJECT .. ' ORDER BY uri LIMIT 100',
			{ FOLLOW, actor, subject }
		)
		for i, row in ipairs(rows) do
			follows[i] = row.uri
		end
	end
	return { followers = count.followers, follows = toarray(follows) }
end
