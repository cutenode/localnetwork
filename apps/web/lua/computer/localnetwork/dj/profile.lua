-- Runs before HappyView stores a computer.localnetwork.dj.profile event: a DJ's playing stats, which locallibrary
-- publishes and project pages and press kits show. Jetstream sends every account's, and all of them are kept, since
-- anyone can come to manage a project; the app only asks for its managers'. It reads them leniently, a section at a
-- time, so they're kept as written. There's one per account, at the key `self`.

function handle()
	if action ~= 'delete' and rkey ~= 'self' then return nil end
	return true
end
