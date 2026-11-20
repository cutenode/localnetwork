-- Runs before HappyView stores a computer.localnetwork.locallibrary.settings event: a DJ's rekordbox My Settings, which
-- locallibrary publishes and reads back on another computer, from any account. Every account's are kept as written.
-- There's one per account, at the key `self`.

function handle(input)
	if input.action ~= 'delete' and input.rkey ~= 'self' then return nil end
	return true
end
