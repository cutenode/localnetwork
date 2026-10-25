-- Runs before HappyView stores a computer.localnetwork.dj.set event. Jetstream sends every account's sets, and all of
-- them are kept, since anyone can come to manage a project; the queries read only managers' sets. What's kept is what
-- the site shows: when and where the set was played, who with, where to listen, and its tracklist as written (tempo,
-- keys and transitions included), plus each named track's title, artists, IDs and start, with the key the support
-- queries group a track by when it has no IDs. A set whose date isn't a YYYY-MM-DD day isn't kept at all. HappyView
-- keeps the event's CID either way.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip.

local ADDRESS = 'community.lexicon.location.address'

local UUID = '^' .. string.rep('%x', 8) .. string.rep('%-' .. string.rep('%x', 4), 3) .. '%-'
	.. string.rep('%x', 12) .. '$'

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
	['125ec42a-7229-4250-afc5-e057484327fe'] = true
}

local function text(value)
	return type(value) == 'string' and value or nil
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

-- The artists a track credits: one by one when the writer told them apart, otherwise its whole credit as one.
local function credits(value, credit)
	local out = {}
	if type(value.artists) == 'table' then
		for _, artist in ipairs(value.artists) do
			local name = type(artist) == 'table' and text(artist.artistName)
			local key = name and normalize(name)
			local id = name and mbid(artist.artistMbId)
			if key and key ~= '' and not PLACEHOLDER[key] and not PLACEHOLDER[id] then
				out[#out + 1] = { artistName = name, artistMbId = id, key = key }
			end
		end
	end
	local key = credit and normalize(credit)
	if #out == 0 and key and key ~= '' and not PLACEHOLDER[key] then out[1] = { artistName = credit, key = key } end
	return toarray(out)
end

-- What the support queries need of a track, or nil for one without a real title and artist, which nobody could be
-- credited for.
local function track(value)
	if type(value) ~= 'table' or value.unidentified == true then return nil end
	local title = text(value.title)
	local key = title and normalize(title)
	if not key or key == '' or PLACEHOLDER[key] then return nil end
	local credit = text(value.artist)
	local artists = credits(value, credit)
	if #artists == 0 then return nil end
	-- Without a credit line that names someone, the artists it names stand in for one, so two tracks with the same
	-- title by different artists stay apart.
	local line = credit and normalize(credit)
	if not line or line == '' or PLACEHOLDER[line] then
		local keys = {}
		for i, artist in ipairs(artists) do keys[i] = artist.key end
		line = table.concat(keys, ', ')
	end
	local start = value.startSeconds
	return {
		title = title,
		artist = credit,
		artists = artists,
		recordingMbId = mbid(value.recordingMbId),
		isrc = isrc(value.isrc),
		startSeconds = math.type(start) == 'integer' and start >= 0 and start or nil,
		-- A newline can't survive normalize(), so it can't be part of either half.
		key = line .. '\n' .. key
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
	for _, item in ipairs(value.tracks) do named[#named + 1] = track(item) end
	local out = {
		date = value.date,
		title = text(value.title),
		kind = text(value.kind),
		venue = text(value.venue),
		trackCount = #value.tracks,
		-- The app reads these leniently, a section at a time, so they're kept as written.
		tracks = toarray(value.tracks),
		playedWith = type(value.playedWith) == 'table' and toarray(value.playedWith) or nil,
		links = type(value.links) == 'table' and toarray(value.links) or nil,
		event = text(value.event),
		namedTracks = toarray(named)
	}
	local place = value.location
	if type(place) == 'table' and place['$type'] == ADDRESS and text(place.country) then
		out.location = {
			country = place.country,
			name = text(place.name),
			locality = text(place.locality),
			region = text(place.region)
		}
	end
	return out
end

local function gate()
	if action == 'delete' then return true end
	local out, why = view(record)
	if not out then log('skipped ' .. uri .. ': ' .. why) end
	return out
end

function handle()
	local ok, result = pcall(gate)
	if ok then return result end
	log('skipped ' .. uri .. ': ' .. tostring(result))
	return nil
end
