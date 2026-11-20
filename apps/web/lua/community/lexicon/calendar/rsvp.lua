-- Runs before HappyView stores a community.lexicon.calendar.rsvp event. Jetstream sends every account's RSVPs, from
-- every calendar app, so only RSVPs to the root's calendar events (published public party events' copies) get in,
-- keeping just the event and the answer. The check is on the subject's URI, not on HappyView having the event, so an
-- RSVP that arrives before its event isn't lost.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local log = require('internal.logging')

local EVENTS = 'at://' .. ROOT_DID .. '/community.lexicon.calendar.event/'

local function gate(input)
	if input.action == 'delete' then return true end
	local subject, status = input.record.subject, input.record.status
	if type(subject) ~= 'table' or type(subject.uri) ~= 'string' or type(subject.cid) ~= 'string' then return nil end
	if type(status) ~= 'string' or string.sub(subject.uri, 1, #EVENTS) ~= EVENTS then return nil end
	return { subject = { uri = subject.uri, cid = subject.cid }, status = status }
end

function handle(input)
	local ok, result = pcall(gate, input)
	if ok then return result end
	log.info('skipped ' .. input.uri .. ': ' .. tostring(result))
	return nil
end
