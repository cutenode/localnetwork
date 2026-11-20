-- Runs before HappyView stores an app.bsky.actor.profile event. Every account's Bluesky profile is kept, at the key
-- `self`, as written: the hosted instance backfills all of them and Jetstream keeps them current, so any account's
-- display name and pronouns are there when the site asks, including an account that only becomes a manager, gets
-- verified or makes a profile here later. The site reads only those two fields (profile pages show the pronouns, and
-- verification statuses compare the names). Answering true keeps the record HappyView already has, with no copy of
-- this instance's own.

function handle(input)
	if input.rkey ~= 'self' then return nil end
	return true
end
