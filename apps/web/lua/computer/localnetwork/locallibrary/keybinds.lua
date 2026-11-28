-- Runs before HappyView stores a computer.localnetwork.locallibrary.keybinds event: the keys someone set in
-- locallibrary's player, which it loads on their other computers and anyone can import. Every account's are kept
-- as written. There's one per account, at the key `self`.

function handle(input)
	if input.action ~= 'delete' and input.rkey ~= 'self' then return nil end
	return true
end
