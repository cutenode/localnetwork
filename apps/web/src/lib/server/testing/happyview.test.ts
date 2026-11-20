import type { JsonValue } from '@atproto/lex-json';
import { parseLexiconDoc } from '@atproto/lexicon';
import { describe, expect, it } from 'vitest';
import { z } from 'zod';
import type { JsonRecord } from '../atproto/repo-client';
import { FakeHappyView } from './happyview';

const probe = parseLexiconDoc({
	lexicon: 1,
	id: 'test.fake.probe',
	defs: {
		main: {
			type: 'query',
			parameters: {
				type: 'params',
				properties: {
					limit: { type: 'integer' },
					size: { type: 'integer', default: 10 },
					flag: { type: 'boolean' },
					tags: { type: 'array', items: { type: 'string' }, maxLength: 2 },
					q: { type: 'string' }
				}
			}
		}
	}
});

const thing = parseLexiconDoc({
	lexicon: 1,
	id: 'test.fake.thing',
	defs: { main: { type: 'record', key: 'any', record: { type: 'object', properties: {} } } }
});

/** Reports what a query script sees, or misbehaves on request. */
const PROBE = `local json = require('internal.json')
local db = require('happyview.db')
local sql = require('happyview.sql')

function handle(input, ctx)
	if input.mode == 'loop' then while true do end end
	if input.mode == 'caught' then pcall(function() while true do end end) return {} end
	if input.mode == 'fail' then error('broken') end
	if input.mode == 'function' then return { f = print } end
	if input.mode == 'internal' then return sql.raw('SELECT count(*) AS n FROM happyview_backfill_jobs') end
	if input.mode == 'table' then return sql.raw('SELECT count(*) AS n FROM happyview_records_v2') end
	if input.mode == 'mixed' then return sql.raw('SELECT count(*) AS n FROM happyview_records, happyview_lexicons') end
	if input.mode == 'materialized' then
		return sql.raw('WITH x AS MATERIALIZED (SELECT uri FROM happyview_records) SELECT uri FROM x', json.to_array({}))
	end
	if input.mode == 'empty' then return sql.raw('SELECT 1 AS one', {}) end
	if input.mode == 'marks' then return sql.raw('SELECT ? AS one', { 1 }) end
	if input.mode == 'global' then return { uri = uri } end
	if input.mode == 'module' then return require('happyview.nope') end
	if input.mode == 'http' then
		local http = require('happyview.http')
		local reached, response = pcall(http.get, 'https://example.com/a')
		return { reached = reached, response = reached and response or tostring(response) }
	end
	if input.mode == 'unlisted' then
		return sql.raw('SELECT jsonb_typeof(record::jsonb) AS kind FROM happyview_records', json.to_array({}))
	end
	if input.mode == 'denied' then return sql.raw("SELECT set_config('role', 'x', true) AS x", json.to_array({})) end
	if input.mode == 'badop' then return db.records('test.fake.thing'):where('n', '~', 1):count() end
	if input.mode == 'db' then
		local things = function() return db.records('test.fake.thing') end
		local first = things():sort('n'):limit(3):run()
		local rest = things():sort('n', 'desc'):limit(3):cursor(first.cursor):run()
		return {
			first = first,
			rest = rest,
			all = things():where('kind', '=', nil):limit(nil):run(),
			full = things():limit(4):run(),
			after = things():limit(4):cursor(things():limit(4):run().cursor):run(),
			count = things():count(),
			strings = things():where('n', '=', '2'):count(),
			numbers = things():where('n', '>=', 2):count(),
			nested = things():where('tags[0]', '=', 'b'):count(),
			deep = things():where('meta.on', '=', true):count(),
			unlike = things():where('kind', 'not like', 'o%'):count(),
			insensitive = things():where('kind', 'ilike', 'ODD'):count(),
			theirs = things():did('did:plc:other'):count(),
			top = things():sort('n', 'asc'):first(),
			none = things():where('kind', '=', 'none'):first(),
		}
	end
	return {
		input = input,
		ctx = ctx,
		types = { limit = math.type(input.limit) or type(input.limit), flag = type(input.flag) },
		decoded = json.decode('{"empty":[],"object":{},"list":[1,2.5,"x",true,null]}'),
		encoded = json.decode(json.encode(json.decode('{"empty":[],"object":{}}'))),
		built = {},
		marked = json.to_array({}),
		copied = json.to_array({ 'a', 'b', nil, 'd' }),
		once = require('internal.json') == json,
		rows = sql.raw(
			"SELECT 1 AS one, 1.5::float8 AS half, 1.5 AS numeric, NULL AS none, $1 AS text, $2::boolean AS yes, coalesce(lower('A'), 'b') AS allowed",
			{ 'hi', true }
		),
		none = sql.raw('SELECT 1 AS one', json.to_array({})),
		stored = sql.raw('SELECT rkey, record FROM happyview_records ORDER BY rkey'),
		sandbox = { io = type(io), load = type(load), execute = type(os.execute), time = type(os.time) },
	}
end`;

/** Decides by rkey what happens to a record event. */
const INDEX = `local log = require('internal.logging')

function handle(input, ctx)
	log.info(input.action .. ' ' .. input.rkey, { trigger = ctx.trigger })
	if input.rkey == 'skip' then return nil end
	if input.rkey == 'keep' and input.action == 'delete' then return nil end
	if input.rkey == 'replace' then return { replaced = input.record.value } end
	if input.rkey == 'context' then return { input = input, ctx = ctx } end
	if input.rkey == 'fail' then error('broken') end
	return true
end`;

function fake(): FakeHappyView {
	return new FakeHappyView('did:plc:root', {
		lexicons: [probe, thing],
		scripts: [
			{ id: 'xrpc.query:test.fake.probe', body: PROBE },
			{ id: 'record.index:test.fake.thing', body: INDEX }
		]
	});
}

async function call(hv: FakeHappyView, query: string, nsid = 'test.fake.probe') {
	const response = await hv.fetch(`http://happyview.test/xrpc/${nsid}?${query}`);
	// SAFETY: the probe script and the fake's own errors always answer with a JSON object.
	const body = (await response.json()) as Record<string, JsonValue>;

	return { status: response.status, body };
}

const write = (hv: FakeHappyView, rkey: string, record: { value: string } | null, collection = 'test.fake.thing') =>
	hv.ingest({ did: 'did:plc:root', collection, rkey, cid: 'bafy', record });

describe('FakeHappyView', () => {
	it('parses, coerces and checks params against the lexicon like HappyView', async () => {
		const hv = fake();

		expect((await call(hv, 'limit=7&flag=1&tags=a&q=a+b%20c%2B')).body).toMatchObject({
			input: { limit: 7, size: 10, flag: true, tags: ['a'], q: 'a b c+' },
			ctx: { trigger: 'xrpc.query:test.fake.probe', has_pds_auth: false, env: {}, method: 'test.fake.probe' },
			types: { limit: 'integer', flag: 'boolean' }
		});
		expect((await call(hv, 'size=3&flag=false&tags=a&tags=b&bare')).body).toMatchObject({
			input: { size: 3, flag: false, tags: ['a', 'b'], bare: '' }
		});

		for (const [query, message] of [
			['limit=x', 'limit must be an integer'],
			['flag=maybe', 'flag must be a boolean'],
			['tags=a&tags=b&tags=c', 'tags must not have more than 2 elements'],
			['q=a&q=b', 'q must be a string']
		])
			expect(await call(hv, query)).toEqual({ status: 400, body: { error: 'InvalidRequest', message } });
	});

	it('converts JSON, arrays, and rows like mlua and sqlx', async () => {
		const { body } = await call(fake(), '');

		expect(body).toMatchObject({
			decoded: { empty: [], object: {}, list: [1, 2.5, 'x', true, null] },
			built: {},
			encoded: { empty: [], object: {} },
			marked: [],
			copied: ['a', 'b'],
			once: true,
			rows: [{ one: 1, half: 1.5, text: 'hi', yes: true, allowed: 'a' }],
			none: [{ one: 1 }],
			sandbox: { io: 'nil', load: 'nil', execute: 'nil', time: 'function' }
		});
	});

	it('stores what the record script lets through', async () => {
		const hv = fake();

		for (const rkey of ['plain', 'skip', 'replace', 'fail', 'keep']) await write(hv, rkey, { value: rkey });
		await write(hv, 'plain', { value: 'again' });
		await write(hv, 'keep', null);
		await write(hv, 'replace', null);
		await write(hv, 'other', { value: 'untracked' }, 'test.fake.other');

		expect((await call(hv, '')).body.stored).toEqual([
			{ rkey: 'fail', record: '{"value":"fail"}' },
			{ rkey: 'keep', record: '{"value":"keep"}' },
			{ rkey: 'plain', record: '{"value":"again"}' }
		]);
		expect(hv.logs).toEqual([
			'create plain',
			'create skip',
			'create replace',
			'create fail',
			'create keep',
			'update plain',
			'delete keep',
			'delete replace'
		]);
	});

	it('answers failed scripts the way HappyView does', async () => {
		const hv = fake();

		expect(await call(hv, 'mode=loop')).toMatchObject({ status: 408, body: { errorType: 'timeout' } });
		expect(hv.instructions).toBe(1_000_000);
		// A pcall can't save a run past the limit.
		expect(await call(hv, 'mode=caught')).toMatchObject({ status: 408, body: { errorType: 'timeout' } });
		expect(await call(hv, 'mode=fail')).toMatchObject({
			status: 500,
			body: { error: 'script_error', errorType: 'runtime', message: expect.stringContaining('broken') }
		});
		expect(await call(hv, 'mode=function')).toMatchObject({
			status: 500,
			body: { errorType: 'runtime', message: expect.stringContaining('unsupported value type') }
		});
		expect(await call(hv, 'mode=internal')).toMatchObject({
			status: 500,
			body: {
				message: expect.stringContaining(
					"cannot reference the protected internal HappyView table 'happyview_backfill_jobs'"
				)
			}
		});
		expect(await call(hv, 'mode=table')).toMatchObject({
			status: 500,
			body: { message: expect.stringContaining("protected internal HappyView table 'happyview_records_v2'") }
		});
		expect(await call(hv, 'mode=mixed')).toMatchObject({
			status: 500,
			body: { message: expect.stringContaining('FORBIDDEN: a query that reads the record views reads no other table') }
		});
		expect(await call(hv, 'mode=materialized')).toMatchObject({
			status: 500,
			body: { message: expect.stringContaining('FORBIDDEN: writes to records of a collection stored outside') }
		});
		expect(await call(hv, 'mode=empty')).toMatchObject({
			status: 500,
			body: { message: expect.stringContaining('params must be an array') }
		});
		expect(await call(hv, 'mode=marks')).toMatchObject({
			status: 500,
			body: { message: expect.stringContaining('syntax error') }
		});
		expect(await call(hv, 'mode=global')).toMatchObject({
			status: 500,
			body: { errorType: 'runtime', message: expect.stringContaining("the 'uri' global was removed in v3") }
		});
		expect(await call(hv, 'mode=module')).toMatchObject({
			status: 500,
			body: { message: expect.stringContaining("module 'happyview.nope' not found") }
		});
		expect(await call(hv, 'mode=unlisted')).toMatchObject({
			status: 500,
			body: {
				message: expect.stringContaining(
					"raw SQL cannot call the function 'jsonb_typeof'; it is not on the plugin SQL allowlist"
				)
			}
		});
		expect(await call(hv, 'mode=denied')).toMatchObject({
			status: 500,
			// Refused outright, before the allowlist, which it isn't on either.
			body: { message: expect.stringMatching(/raw SQL cannot call the function 'set_config'$/) }
		});
		expect(await call(hv, 'mode=badop')).toMatchObject({
			status: 500,
			body: { message: expect.stringContaining("BAD_INPUT: invalid filter op '~'") }
		});
		expect((await call(hv, '', 'test.fake.thing')).status).toBe(404);
	});

	it("reads records through happyview.db's chains: filters, sorts, pages, counts and first", async () => {
		const hv = fake();

		const seeded = (did: string, rkey: string, record: JsonRecord) => ({
			did,
			collection: 'test.fake.thing',
			rkey,
			cid: 'bafy',
			record
		});

		await hv.seed([
			seeded('did:plc:root', 'a', { n: 1, kind: 'odd', tags: ['a'], meta: { on: true } }),
			seeded('did:plc:root', 'b', { n: 2, kind: 'even', tags: ['b'] }),
			seeded('did:plc:root', 'c', { n: '2', kind: 'text' }),
			seeded('did:plc:other', 'd', { n: 3, kind: 'Odd', tags: ['b', 'c'] })
		]);

		const { body } = await call(hv, 'mode=db');
		const page = z.object({ records: z.array(z.object({ rkey: z.string() })) });
		const pages = z.object({ first: page, rest: page, all: page }).parse(body);
		const rkeys = ({ records }: z.infer<typeof page>) => records.map(({ rkey }) => rkey);

		expect(body).toMatchObject({
			first: { cursor: expect.any(String) },
			count: 4,
			strings: 1,
			numbers: 2,
			nested: 2,
			deep: 1,
			unlike: 3,
			insensitive: 2,
			theirs: 1,
			top: { uri: 'at://did:plc:root/test.fake.thing/c', did: 'did:plc:root', rkey: 'c', record: { n: '2' } }
		});
		// Sorting compares jsonb values, descending unless asked otherwise, where a string comes before any number. A full
		// page carries a cursor, as HappyView's does.
		expect([rkeys(pages.first), rkeys(pages.rest)]).toEqual([['d', 'b', 'a'], ['c']]);
		expect(body.rest).not.toHaveProperty('cursor');
		expect(rkeys(pages.all)).toHaveLength(4);
		// A page that's exactly full still has a cursor, to an empty page.
		expect(body).toMatchObject({ full: { cursor: expect.any(String) }, after: { records: [] } });
		expect(body.after).not.toHaveProperty('cursor');
		expect(body).not.toHaveProperty('none');
	});

	it('answers HTTP from `web`, and raises for a URL nothing answers, without the network', async () => {
		const hv = fake();

		expect((await call(hv, 'mode=http')).body).toEqual({
			reached: false,
			response: expect.stringContaining('could not connect to https://example.com/a')
		});
		hv.web = (url) => (url === 'https://example.com/a' ? { status: 200, body: 'hi' } : undefined);
		expect((await call(hv, 'mode=http')).body).toEqual({
			reached: true,
			response: { status: 200, body: 'hi', headers: {} }
		});
		expect(hv.requests).toEqual(['https://example.com/a', 'https://example.com/a']);
	});

	it("re-reads one account's records through the record script in a backfill, and says when it's done", async () => {
		const hv = fake();

		const admin = (path: string, body?: { collection: string; did: string }) =>
			hv.fetch(`http://happyview.test${path}`, body && { method: 'POST', body: JSON.stringify(body) });

		expect((await admin('/admin/backfill', { collection: 'test.fake.thing', did: 'did:plc:root' })).status).toBe(404);
		hv.repo = (did, collection) =>
			['plain', 'skip'].map((rkey) => ({ did, collection, rkey, cid: 'bafy', record: { value: rkey } }));

		expect(
			await (await admin('/admin/backfill', { collection: 'test.fake.thing', did: 'did:plc:root' })).json()
		).toEqual({
			id: 'job-0',
			status: 'running'
		});
		expect(await (await admin('/admin/backfill/status')).json()).toEqual([{ id: 'job-0', status: 'completed' }]);
		expect((await call(hv, '')).body.stored).toEqual([{ rkey: 'plain', record: '{"value":"plain"}' }]);
	});

	it('re-reads a list of accounts, with `did` added to them, as one backfill job', async () => {
		const hv = fake();
		hv.repo = (did, collection) => [{ did, collection, rkey: did.slice(-1), cid: 'bafy', record: { value: did } }];

		const backfill = (body: { collection: string; dids: string[]; did?: string }) =>
			hv.fetch('http://happyview.test/admin/backfill', { method: 'POST', body: JSON.stringify(body) });

		const response = await backfill({
			collection: 'test.fake.thing',
			dids: ['did:plc:a', 'did:plc:b'],
			did: 'did:plc:a'
		});

		expect({ status: response.status, body: await response.json() }).toEqual({
			status: 201,
			body: { id: 'job-0', status: 'running' }
		});
		expect((await call(hv, '')).body.stored).toEqual([
			{ rkey: 'a', record: '{"value":"did:plc:a"}' },
			{ rkey: 'b', record: '{"value":"did:plc:b"}' }
		]);
		expect(hv.logs).toEqual(['create a', 'create b']);
		await expect(
			backfill({ collection: 'test.fake.thing', dids: Array.from({ length: 501 }, (_, i) => `did:plc:${i}`) })
		).rejects.toThrow('500');
	});

	it('gives a record script the event and its context, and nothing in place of a deleted record', async () => {
		const hv = fake();
		await write(hv, 'context', { value: 'x' });

		const [stored] = z.array(z.object({ record: z.string() })).parse((await call(hv, '')).body.stored);

		expect(JSON.parse(stored.record)).toEqual({
			input: {
				action: 'create',
				uri: 'at://did:plc:root/test.fake.thing/context',
				did: 'did:plc:root',
				collection: 'test.fake.thing',
				rkey: 'context',
				record: { value: 'x' }
			},
			ctx: {
				trigger: 'record.index:test.fake.thing',
				caller_did: 'did:plc:root',
				has_pds_auth: false,
				env: {},
				collection: 'test.fake.thing'
			}
		});
		await write(hv, 'context', null);
		expect((await call(hv, '')).body.stored).toEqual([]);
		expect(hv.logs).toEqual(['create context', 'delete context']);
	});

	it('refuses a script that still reads a global HappyView 3 removed, as its upload does', () => {
		const bundle = (body: string, id = 'xrpc.query:test.fake.probe') => ({
			lexicons: [probe],
			scripts: [{ id, body }]
		});

		expect(
			() => new FakeHappyView('did:plc:root', bundle(PROBE.replace("require('happyview.sql')", 'toarray')))
		).toThrow('xrpc.query:test.fake.probe: script references removed globals: toarray');
		expect(
			() => new FakeHappyView('did:plc:root', bundle('function handle() log(params.q) return toarray({}) end'))
		).toThrow('script references removed globals: log, toarray, params');
		// A record's names were never globals in a query, and the probe's own locals and parameters aren't globals at all.
		expect(() => new FakeHappyView('did:plc:root', bundle(PROBE))).not.toThrow();
		expect(
			() => new FakeHappyView('did:plc:root', bundle('function handle() return rkey end', 'record.index:x'))
		).toThrow('record.index:x: script references removed globals: rkey');
	});

	it('refuses a script whose top level needs a module, which its upload loads without one', async () => {
		const body = `local json = require('internal.json')
local TAGS = json.decode('["a"]')
local FIRST = TAGS[1]:upper()
function handle() return { first = FIRST } end`;

		const hv = new FakeHappyView('did:plc:root', {
			lexicons: [probe],
			scripts: [{ id: 'xrpc.query:test.fake.probe', body }]
		});

		await expect(call(hv, '')).rejects.toThrow('xrpc.query:test.fake.probe: script compilation failed');
	});
});
