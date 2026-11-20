-- Runs before HappyView stores a computer.localnetwork.listing.profile event: staff listing a DJ, which only the root
-- account writes. Anyone can write this collection to their own repo, so only the root's get in, and only ones keyed
-- by the DID they name, since searchPerformers finds the profile by the key.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local log = require('internal.logging')

local function gate(input)
	if input.did ~= ROOT_DID then return nil end
	if input.action == 'delete' then return true end
	if not (input.record.subject == input.rkey and type(input.record.createdAt) == 'string') then return nil end
	return true
end

function handle(input)
	local ok, result = pcall(gate, input)
	if ok then return result end
	log.info('skipped ' .. input.uri .. ': ' .. tostring(result))
	return nil
end
