-- Runs before HappyView stores a computer.localnetwork.listing.profile event: staff listing a DJ, which only the root
-- account writes. Anyone can write this collection to their own repo, so only the root's get in, and only ones keyed
-- by the DID they name, since searchPerformers finds the profile by the key.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local function gate()
	if did ~= ROOT_DID then return nil end
	if action == 'delete' then return true end
	if not (record.subject == rkey and type(record.createdAt) == 'string') then return nil end
	return true
end

function handle()
	local ok, result = pcall(gate)
	if ok then return result end
	log('skipped ' .. uri .. ': ' .. tostring(result))
	return nil
end
