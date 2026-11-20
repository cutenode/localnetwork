-- Runs before HappyView stores an app.bsky.graph.follow event. Jetstream sends every Bluesky follow on the network
-- (about 40 a second), and the site only reads the follows of the managers of a project with a press kit that treats
-- people they follow differently from anyone signed in (the "following" audience rule, which any of its managers'
-- follows meet), so only theirs get in, keeping just the subject and when. It runs for every follow on the network,
-- and HappyView's record store answers quickly only when a query names the accounts it reads, so it first looks for
-- root projects the follower manages (none, for nearly every follow), then for press kits of those, by the root or one
-- of their managers (the ones the epk script keeps), that read follows.
-- HappyView stores the whole record when a script errors, so the gate runs under pcall and turns errors into a skip. A
-- require that fails while the script loads would happen outside it, so the gate requires the SQL library itself.

local json = require('internal.json')
local log = require('internal.logging')

local EPK = 'computer.localnetwork.epk'
local PROJECT = 'computer.localnetwork.project'

-- Postgres placeholders for `values`, numbered from `first`, joined with commas.
local function marks(values, first)
	local out = {}
	for i = 1, #values do
		out[i] = '$' .. (first + i - 1)
	end
	return table.concat(out, ', ')
end

local function gate(input)
	if input.action == 'delete' then return true end
	if not (type(input.record.subject) == 'string' and type(input.record.createdAt) == 'string') then return nil end
	local sql = require('happyview.sql')
	local projects = sql.raw(
		'SELECT uri, record FROM happyview_records WHERE did = $1 AND collection = $2'
			.. " AND record::jsonb -> 'managers' @> jsonb_build_array($3::text)",
		json.to_array({ ROOT_DID, PROJECT, input.did })
	)
	if #projects == 0 then return nil end
	local uris, authors, seen = {}, { ROOT_DID }, { [ROOT_DID] = true }
	for i, project in ipairs(projects) do
		uris[i] = project.uri
		for _, manager in ipairs(json.decode(project.record).managers) do
			if not seen[manager] then
				seen[manager] = true
				authors[#authors + 1] = manager
			end
		end
	end
	local kits = sql.raw(
		'SELECT 1 AS found FROM happyview_records WHERE did IN ('
			.. marks(authors, 2)
			.. ') AND collection = $1'
			.. " AND record::jsonb ->> 'project' IN ("
			.. marks(uris, #authors + 2)
			.. ')'
			.. " AND record::jsonb #> '{access,following}' IS DISTINCT FROM record::jsonb #> '{access,anyone}' LIMIT 1",
		json.to_array(table.move(uris, 1, #uris, #authors + 2, table.move(authors, 1, #authors, 2, { EPK })))
	)
	if #kits == 0 then return nil end
	return { subject = input.record.subject, createdAt = input.record.createdAt }
end

function handle(input)
	local ok, result = pcall(gate, input)
	if ok then return result end
	log.info('skipped ' .. input.uri .. ': ' .. tostring(result))
	return nil
end
