-- Records the index lacks, read straight from their account's own PDS, so DJ stats, a calendar event or the bug
-- report space HappyView hasn't heard of yet still answers, without a backfill. Bluesky profiles aren't among them:
-- the store holds every one, and the profile gate keeps only the name and pronouns. The read goes to the PDS
-- with HTTP, never through HappyView's own com.atproto.repo.getRecord, which would index (and keep) everything it
-- reads. Nothing read here is written back, and the record scripts' gates don't run on it: the app reads only the
-- fields it uses. Any failure (a DID that isn't did:plc or did:web, no DID document or PDS, an answer other than 200
-- with a JSON object) is a miss, as it is for a record the index lacks.

local json = require('internal.json')

-- The collections a read goes through to the PDS for. The queries are public, so they mustn't fetch whatever anyone
-- names.
local READ_THROUGH = {
	['community.lexicon.calendar.event'] = true,
	['app.userinput.space'] = true,
	['computer.localnetwork.dj.profile'] = true,
}

-- `value` escaped for a URL's query string.
local function escape(value)
	return (value:gsub('[^%w%-%._~]', function(char)
		return string.format('%%%02X', char:byte())
	end))
end

-- Whether `collection` is one the reads go through to the PDS for.
local function readsThrough(collection)
	return READ_THROUGH[collection] == true
end

-- A reader of PDS records for one request, which reads each DID's document once. It returns nil without HappyView's
-- HTTP library. The library is required here, under pcall, rather than at the top: a query whose top-level require
-- fails doesn't load at all, and every query loads this file.
local function pdsReader()
	local loaded, http = pcall(require, 'happyview.http')
	if not loaded then return nil end

	-- `url`'s JSON object, or nil.
	local function get(url)
		local ok, response = pcall(http.get, url)
		if not (ok and type(response) == 'table' and response.status == 200 and type(response.body) == 'string') then
			return nil
		end
		local decoded, body = pcall(json.decode, response.body)
		return decoded and type(body) == 'table' and body or nil
	end

	-- The account's PDS, from its DID document's #atproto_pds service.
	local function endpoint(did)
		local host = did:match('^did:web:([%w%.%-]+)$')
		local url = did:match('^did:plc:[a-z2-7]+$') and 'https://plc.directory/' .. did
			or host and 'https://' .. host .. '/.well-known/did.json'
		local document = url and get(url)
		for _, service in ipairs(document and type(document.service) == 'table' and document.service or {}) do
			local id = type(service) == 'table' and service.id
			local at = type(service) == 'table' and type(service.serviceEndpoint) == 'string' and service.serviceEndpoint
			if
				(id == '#atproto_pds' or id == did .. '#atproto_pds')
				and service.type == 'AtprotoPersonalDataServer'
				and at
				and at:match('^https?://[^/]')
			then
				return (at:gsub('/+$', ''))
			end
		end
		return false
	end

	local endpoints = {}
	local reader = {}

	-- The PDS's answer to com.atproto.repo.`method` from `did`'s repo with `params` (the repo added), or nil.
	function reader.call(did, method, params)
		if endpoints[did] == nil then endpoints[did] = endpoint(did) end
		if not endpoints[did] then return nil end
		local query = { 'repo=' .. escape(did) }
		for _, name in ipairs(params) do
			query[#query + 1] = name[1] .. '=' .. escape(tostring(name[2]))
		end
		return get(endpoints[did] .. '/xrpc/com.atproto.repo.' .. method .. '?' .. table.concat(query, '&'))
	end

	-- A record the PDS answers with (`{ uri, cid, value }`) as a recordView, or nil when it isn't one.
	function reader.view(record)
		local isRecord = type(record) == 'table' and type(record.uri) == 'string' and type(record.cid) == 'string'
		if not (isRecord and type(record.value) == 'table') then return nil end
		return { uri = record.uri, cid = record.cid, value = record.value }
	end

	return reader
end

-- The records at `uris` as recordViews by URI: the index's, then, for a URI it lacks whose collection the reads go
-- through for, its account's PDS's. A URI found nowhere is left out.
local function fetchThrough(uris)
	local where, values = atUris(uris, '')
	local found = {}
	for _, row in ipairs(query('SELECT uri, cid, record FROM happyview_records WHERE ' .. where, values)) do
		found[row.uri] = recordView(row)
	end
	local reader
	for _, uri in ipairs(uris) do
		local did, collection, rkey = uri:match('^at://(did:[^/]+)/([^/]+)/([%w%.%-_:~]+)$')
		if not found[uri] and did and readsThrough(collection) then
			reader = reader or pdsReader()
			if not reader then break end
			local record = reader.view(reader.call(did, 'getRecord', { { 'collection', collection }, { 'rkey', rkey } }))
			found[uri] = record and record.uri == uri and record or nil
		end
	end
	return found
end

-- `did`'s records of `collection` from its PDS, as listRecords pages the index: last record key first, at most
-- `limit`, after `cursor` (the last record key of the page before), with the cursor for the next page when there is
-- one. nil when the reads don't go through for the collection or the PDS doesn't answer. A PDS lists newest first
-- unless asked to reverse, and its cursor is the last record key it listed.
local function listThrough(did, collection, limit, cursor)
	local reader = did:match('^did:') and readsThrough(collection) and pdsReader()
	if not reader then return nil end
	-- One record more than the page shows whether there's another, short of the 100 a PDS lists at most.
	local params = { { 'collection', collection }, { 'limit', math.min(limit + 1, 100) }, { 'reverse', 'false' } }
	if cursor then params[#params + 1] = { 'cursor', cursor } end
	local page = reader.call(did, 'listRecords', params)
	if not (page and type(page.records) == 'table') then return nil end
	local out = {}
	for _, record in ipairs(page.records) do
		out[#out + 1] = reader.view(record)
	end
	local more = #out > limit or #out == 100 and page.cursor ~= nil
	local records = table.move(out, 1, math.min(#out, limit), 1, {})
	return records, more and records[#records].uri:match('[^/]+$') or nil
end
