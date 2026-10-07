import type { JsonValue } from '@atproto/lex-json';
import { describe, expect, it } from 'vitest';
import { HappyViewClient, HappyViewError } from './client';

const URL_BASE = 'http://happyview.test';

/** A HappyView that answers every request with `body`, and remembers what it was asked. */
function serving(body: JsonValue, status = 200) {
	const requests: Request[] = [];

	const fetch: typeof globalThis.fetch = async (input, init) => {
		requests.push(new Request(input, init));

		return Response.json(body, { status });
	};

	return { requests, fetch };
}

describe('HappyViewClient', () => {
	it('sends params the way XRPC does, with arrays as repeated keys', async () => {
		const happyView = serving({ projects: [], total: 0 });
		const client = new HappyViewClient({ url: URL_BASE, fetch: happyView.fetch });

		await client.query('searchProjects', {
			q: 'deep house',
			latE6: 52_520_000,
			genres: ['deep house', 'techno'],
			kinds: [],
			includeInactive: true,
			country: undefined
		});

		// HappyView percent-decodes without reading `+` as a space.
		expect(happyView.requests[0].url).toBe(
			`${URL_BASE}/xrpc/computer.localnetwork.directory.searchProjects?q=deep%20house&latE6=52520000&genres=deep%20house&genres=techno&includeInactive=true`
		);
	});

	it('refuses params its lexicon does not allow, before asking', async () => {
		const happyView = serving({ projects: [], total: 0 });
		const client = new HappyViewClient({ url: URL_BASE, fetch: happyView.fetch });

		await expect(client.query('searchProjects', { limit: 500 })).rejects.toThrow(/limit/);
		await expect(client.query('getProjects', { uris: ['not a uri'] })).rejects.toThrow(/uris/);
		expect(happyView.requests).toEqual([]);
	});

	it('rejects answers that do not match the lexicon', async () => {
		const client = (body: JsonValue) => new HappyViewClient({ url: URL_BASE, fetch: serving(body).fetch });

		await expect(client({ projects: 'none' }).query('getProjects', { uris: [] })).rejects.toThrow(/projects/);
		await expect(client({ total: 1 }).query('searchProjects')).rejects.toThrow(/projects/);
		await expect(
			client({ project: { uri: 'at://did:plc:root/computer.localnetwork.project/1', cid: 'nope', record: {} } }).query(
				'getProject',
				{ slug: 'x' }
			)
		).rejects.toThrow(/cid/);
	});

	it('reports HTTP failures with their status', async () => {
		const client = new HappyViewClient({
			url: URL_BASE,
			fetch: serving({ error: 'script_error', message: 'boom' }, 500).fetch
		});

		const failure = await client.query('getStats').catch((error: Error) => error);
		expect(failure).toBeInstanceOf(HappyViewError);
		expect(failure).toMatchObject({ status: 500, message: expect.stringContaining('boom') });
	});

	it('reports an unreachable HappyView as unavailable', async () => {
		const client = new HappyViewClient({
			url: URL_BASE,
			fetch: async () => {
				throw new TypeError('fetch failed');
			}
		});

		await expect(client.query('getStats')).rejects.toEqual(
			new HappyViewError(503, 'HappyView /xrpc/computer.localnetwork.directory.getStats: fetch failed')
		);
	});

	it('signs admin calls with the API key and refuses them without one', async () => {
		const happyView = serving([]);
		await new HappyViewClient({ url: URL_BASE, apiKey: 'hv_secret', fetch: happyView.fetch }).scripts();
		expect(happyView.requests[0].headers.get('authorization')).toBe('Bearer hv_secret');

		await expect(new HappyViewClient({ url: URL_BASE, fetch: happyView.fetch }).scripts()).rejects.toThrow(
			/HAPPYVIEW_API_KEY/
		);
		expect(happyView.requests).toHaveLength(1);
	});

	it('says whether HappyView answers', async () => {
		expect(await new HappyViewClient({ url: URL_BASE, fetch: serving({}).fetch }).health()).toBe(true);
		expect(await new HappyViewClient({ url: URL_BASE, fetch: serving({}, 503).fetch }).health()).toBe(false);

		const unreachable = new HappyViewClient({
			url: URL_BASE,
			fetch: () => Promise.reject(new TypeError('fetch failed'))
		});

		expect(await unreachable.health()).toBe(false);
	});
});
