import type { JsonValue } from '@atproto/lex-json';
import { parseLexiconDoc } from '@atproto/lexicon';
import { describe, expect, it } from 'vitest';
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
					flag: { type: 'boolean' },
					tags: { type: 'array', items: { type: 'string' } },
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
const PROBE = `function handle()
	if params.mode == 'loop' then while true do end end
	if params.mode == 'fail' then error('broken') end
	if params.mode == 'function' then return { f = print } end
	return {
		params = params,
		types = { limit = math.type(params.limit) or type(params.limit), flag = type(params.flag) },
		decoded = json.decode('{"empty":[],"object":{},"list":[1,2.5,"x",true,null]}'),
		encoded = json.decode(json.encode(json.decode('{"empty":[],"object":{}}'))),
		built = {},
		marked = toarray({}),
		copied = toarray({ 'a', 'b', nil, 'd' }),
		rows = db.raw('SELECT 1 AS one, 1.5 AS half, NULL AS none, ? AS text, ? AS yes', { 'hi', true }),
		stored = db.raw('SELECT rkey, record FROM happyview_records ORDER BY rkey'),
		sandbox = { io = type(io), load = type(load), execute = type(os.execute), time = type(os.time) },
	}
end`;

/** Decides by rkey what happens to a record event. */
const INDEX = `function handle()
	log(action .. ' ' .. rkey)
	if rkey == 'skip' then return nil end
	if rkey == 'keep' and action == 'delete' then return nil end
	if rkey == 'replace' then return { replaced = record.value } end
	if rkey == 'fail' then error('broken') end
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
	it('parses and coerces params like HappyView', async () => {
		const hv = fake();

		expect((await call(hv, 'limit=7&flag=1&tags=a&q=a+b%20c')).body).toMatchObject({
			params: { limit: 7, flag: true, tags: 'a', q: 'a+b c' },
			types: { limit: 'integer', flag: 'boolean' }
		});

		expect((await call(hv, 'limit=x&flag=maybe&tags=a&tags=b&bare')).body).toMatchObject({
			params: { limit: 'x', flag: 'maybe', tags: ['a', 'b'], bare: '' },
			types: { limit: 'string', flag: 'string' }
		});
	});

	it('converts JSON, arrays, and rows like mlua and sqlx', async () => {
		const { body } = await call(fake(), '');

		expect(body).toMatchObject({
			decoded: { empty: [], object: {}, list: [1, 2.5, 'x', true, null] },
			built: {},
			encoded: { empty: [], object: {} },
			marked: [],
			copied: ['a', 'b'],
			rows: [{ one: 1, half: 1.5, text: 'hi', yes: 1 }],
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
		expect(await call(hv, 'mode=fail')).toMatchObject({
			status: 500,
			body: { error: 'script_error', errorType: 'runtime', message: expect.stringContaining('broken') }
		});
		expect(await call(hv, 'mode=function')).toMatchObject({
			status: 500,
			body: { errorType: 'runtime', message: expect.stringContaining('unsupported value type') }
		});
		expect((await call(hv, '', 'test.fake.thing')).status).toBe(404);
	});
});
