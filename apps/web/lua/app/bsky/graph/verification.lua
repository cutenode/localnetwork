-- Runs before HappyView stores an app.bsky.graph.verification event. Jetstream sends every account's verifications,
-- and the site only reads the ones the root account issued, so only the root's get in, as written: the app checks each
-- one when it reads it.

function handle()
	if did ~= ROOT_DID then return nil end
	return true
end
