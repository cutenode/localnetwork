-- Runs before HappyView stores a computer.localnetwork.dj.set event. Jetstream sends every account's sets, and all of
-- them are kept, since anyone can come to manage a project; the queries read only managers' sets. What's kept is what
-- the site shows: when and where the set was played, who with, where to listen (its links, and the record its recording
-- plays from), and its tracklist as written (tempo, keys and transitions included), plus each named track's title,
-- artists, IDs and start, with the key the support queries group a track by when it has no IDs. Its address, sources
-- and createdAt are kept as written too, since locallibrary reads its own sets back through getRecords before replacing
-- them, and keeps a set's first createdAt.
-- A set whose date isn't a YYYY-MM-DD day isn't kept at all. HappyView keeps the event's CID either way.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local log = require('internal.logging')
local json = require('internal.json')

local ADDRESS = 'community.lexicon.location.address'

local UUID = '^'
	.. string.rep('%x', 8)
	.. string.rep('%-' .. string.rep('%x', 4), 3)
	.. '%-'
	.. string.rep('%x', 12)
	.. '$'

-- Credits that name nobody: what taggers write for an unreleased or unknown track, and MusicBrainz's own Various
-- Artists and [unknown] artists. A track credited only to these isn't counted for anyone.
local PLACEHOLDER = {
	['id'] = true,
	['id - id'] = true,
	['unknown'] = true,
	['unknown artist'] = true,
	['[unknown]'] = true,
	['various artists'] = true,
	['89ad4ac3-39f7-470e-963a-56509c546377'] = true,
	['125ec42a-7229-4250-afc5-e057484327fe'] = true,
}

-- `value` when it's a string, and at most `max` bytes long when there's a max. Nothing checks a record against its
-- lexicon before this script, and the directory's views (lexicons/computer/localnetwork/directory/defs.json) cap the
-- strings they take from a set, at dj.set's maxLength where it has one, so a longer one is left out here instead of
-- failing every query that shows it.
local function text(value, max)
	return type(value) == 'string' and (max == nil or #value <= max) and value or nil
end

-- Lowercased, with each run of whitespace as one space and none at the ends, so "Ferrule " and "ferrule" credit one
-- artist. Lowercasing is ASCII only, since Lua has no Unicode case tables: "Ä" and "ä" stay two names.
-- lib/directory.lua normalizes what a query asks for the same way.
local function normalize(name)
	return (name:lower():gsub('%s+', ' '):match('^ ?(.-) ?$'))
end

-- A MusicBrainz ID as a bare lowercase UUID. Writers send `mbid:<uuid>` or the bare UUID (teal.fm's form), and
-- both have to count as one recording or artist.
local function mbid(value)
	value = text(value)
	if not value then return nil end
	return (value:lower():gsub('^mbid:', '')):match(UUID)
end

local function isrc(value)
	value = text(value)
	value = value and value:upper()
	return value and #value == 12 and value:match('^%w+$') or nil
end

-- `s` cut wherever one of `seps` (Lua patterns, matched in lowercase) is, its pieces trimmed of spaces and brackets,
-- without empty ones or a repeat (by normalize()), in order. Lowercasing is ASCII only, so it keeps every byte where
-- it was, and gmatch finds the pieces, since a loop over every byte costs a long tracklist too many instructions.
local function cut(s, seps)
	local marks = s:lower()
	for _, sep in ipairs(seps) do
		marks = marks:gsub(sep, function(m)
			return ('\31'):rep(#m)
		end)
	end
	local out, seen = {}, {}
	for first, after in marks:gmatch('()[^\31]+()') do
		local name = s:sub(first, after - 1):match('^[%s()]*(.-)[%s()]*$')
		local key = normalize(name)
		if key ~= '' and not seen[key] then
			seen[key] = true
			out[#out + 1] = name
		end
	end
	return out
end

-- Where a credit line names one artist after another, as loclib-model's artist_credits splits it: commas, semicolons
-- and featuring credits ("A, B feat. C", "A (ft. B)").
local CREDITS = { ',', ';', '%s*%(?%f[%w]featuring%s+', '%s*%(?%f[%w]feat%.?%s+', '%s*%(?%f[%w]ft%.?%s+' }

-- Where a collaboration's credit names one act after another: "A & B", "A x B", "A vs. B", "A and B", as
-- loclib-model's collab_parts splits them.
local COLLAB = { '%s+&%s+', '%s+x%s+', '%s+vs%.?%s+', '%s+and%s+' }

-- The acts `name` credits when it's a collaboration, each as an artist (a name and its key), or nil when it names
-- one. The queries count it for each act only when every one is credited alone somewhere (lib/directory.lua), so
-- "Chase & Status" stays one act.
local function parts(name)
	local acts = cut(name, COLLAB)
	if #acts < 2 then return nil end
	local out = {}
	for i, act in ipairs(acts) do
		out[i] = { artistName = act, key = normalize(act) }
	end
	return json.to_array(out)
end

-- The artists a track credits: one by one as the writer told them apart, otherwise as its credit line names them. One
-- without a MusicBrainz ID (which says who they are) keeps the acts it could be a collaboration of.
local function credits(value, credit)
	local out = {}
	if type(value.artists) == 'table' then
		for _, artist in ipairs(value.artists) do
			local name = type(artist) == 'table' and text(artist.artistName, 2560)
			local key = name and normalize(name)
			local id = name and mbid(artist.artistMbId)
			if key and key ~= '' and not PLACEHOLDER[key] and not PLACEHOLDER[id] then
				out[#out + 1] = { artistName = name, artistMbId = id, key = key, parts = not id and parts(name) or nil }
			end
		end
	end
	local key = credit and normalize(credit)
	if #out == 0 and key and key ~= '' and not PLACEHOLDER[key] then
		for _, name in ipairs(cut(credit, CREDITS)) do
			key = normalize(name)
			if not PLACEHOLDER[key] then out[#out + 1] = { artistName = name, key = key, parts = parts(name) } end
		end
	end
	return json.to_array(out)
end

-- Every act a credit line names, wherever it puts them, sorted: what a track without IDs is told apart by, so
-- "A & B", "B x A" and "B feat. A" credit one pair.
local ACTS = { table.unpack(CREDITS) }
for _, sep in ipairs(COLLAB) do
	ACTS[#ACTS + 1] = sep
end
local function actsKey(line)
	local acts = cut(line, ACTS)
	for i, act in ipairs(acts) do
		acts[i] = normalize(act)
	end
	table.sort(acts)
	return table.concat(acts, ', ')
end

-- What the support queries need of a track, or nil for one without a real title and artist, which nobody could be
-- credited for.
local function track(value)
	if type(value) ~= 'table' or value.unidentified == true then return nil end
	local title = text(value.title, 3000)
	local key = title and normalize(title)
	if not key or key == '' or PLACEHOLDER[key] then return nil end
	local credit = text(value.artist, 3000)
	local artists = credits(value, credit)
	if #artists == 0 then return nil end
	-- Without a credit line that names someone, the artists it names stand in for one, so two tracks with the same
	-- title by different artists stay apart.
	local line = credit and normalize(credit)
	if not line or line == '' or PLACEHOLDER[line] then
		local keys = {}
		for i, artist in ipairs(artists) do
			keys[i] = artist.key
		end
		line = table.concat(keys, ', ')
	end
	line = actsKey(line)
	-- At most a credit line's length, so a track's id (supportedTrack's, at most 6006 bytes) fits its title too.
	if #line > 3000 then return nil end
	local start = value.startSeconds
	return {
		title = title,
		artist = credit,
		artists = artists,
		recordingMbId = mbid(value.recordingMbId),
		isrc = isrc(value.isrc),
		startSeconds = math.type(start) == 'integer' and start >= 0 and start or nil,
		-- A newline can't survive normalize(), so it can't be part of either half.
		key = line .. '\n' .. key,
	}
end

-- The directory's setView of the record, minus its URI and CID, with what the DJ sets block shows besides and its
-- named tracks, or nil with why it can't be one.
local function view(value)
	local date = text(value.date)
	if not (date and date:match('^%d%d%d%d%-%d%d%-%d%d$') and type(value.tracks) == 'table') then
		return nil, 'bad date or tracks'
	end
	local named = {}
	for _, item in ipairs(value.tracks) do
		named[#named + 1] = track(item)
	end
	local out = {
		date = value.date,
		title = text(value.title, 1000),
		kind = text(value.kind, 640),
		venue = text(value.venue, 1000),
		trackCount = #value.tracks,
		-- The app reads these leniently, a section at a time, so they're kept as written.
		tracks = json.to_array(value.tracks),
		playedWith = type(value.playedWith) == 'table' and json.to_array(value.playedWith) or nil,
		links = type(value.links) == 'table' and json.to_array(value.links) or nil,
		recording = type(value.recording) == 'table' and value.recording or nil,
		sources = type(value.sources) == 'table' and json.to_array(value.sources) or nil,
		event = text(value.event),
		createdAt = text(value.createdAt),
		namedTracks = json.to_array(named),
	}
	local place = value.location
	local isAddress = type(place) == 'table' and place['$type'] == ADDRESS
	if isAddress and text(place.country, 10) and (place.locality == nil or text(place.locality, 1000)) then
		out.location = place
	end
	return out
end

local function gate(input)
	if input.action == 'delete' then return true end
	local out, why = view(input.record)
	if not out then log.info('skipped ' .. input.uri .. ': ' .. why) end
	return out
end

function handle(input)
	local ok, result = pcall(gate, input)
	if ok then return result end
	log.info('skipped ' .. input.uri .. ': ' .. tostring(result))
	return nil
end
