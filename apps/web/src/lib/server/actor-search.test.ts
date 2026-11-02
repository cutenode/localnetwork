import { describe, expect, it, vi } from 'vitest';
import { ActorSearch } from './actor-search';
import { silentLogger } from './logger';

const neon = {
	did: 'did:plc:neon',
	handle: 'neon.bsky.social',
	displayName: 'Neon Tide',
	avatar: 'https://cdn.bsky.app/img/avatar/plain/did:plc:neon/abc@jpeg'
};

describe('ActorSearch', () => {
	it('asks Bluesky for five accounts, keeps only its own avatars, and answers repeats from memory', async () => {
		const fetch = vi.fn<typeof globalThis.fetch>(async () =>
			Response.json({
				actors: [
					neon,
					{
						did: 'did:plc:nell',
						handle: 'nell.test',
						displayName: '  ',
						avatar: 'https://tracker.example/pixel.gif',
						viewer: {}
					}
				]
			})
		);

		const search = new ActorSearch({ logger: silentLogger, provider: 'bluesky', fetch });

		expect(await search.search(' @NE ')).toEqual([
			neon,
			{ did: 'did:plc:nell', handle: 'nell.test', displayName: undefined, avatar: undefined }
		]);

		const [input] = fetch.mock.calls[0];
		const url = new URL(input instanceof Request ? input.url : input);
		expect(`${url.origin}${url.pathname}`).toBe(
			// Checks the search goes to Bluesky's AppView, which actor-search.ts's own ast-grep-ignore allows.
			// ast-grep-ignore: no-direct-pds-read
			'https://public.api.bsky.app/xrpc/app.bsky.actor.searchActorsTypeahead'
		);
		expect(Object.fromEntries(url.searchParams)).toEqual({ q: 'ne', limit: '5' });

		await search.search('ne');
		expect(fetch).toHaveBeenCalledTimes(1);
	});

	it('asks again once an answer is ten minutes old, and forgets the oldest past 500 searches', async () => {
		let now = 0;
		const fetch = vi.fn<typeof globalThis.fetch>(async () => Response.json({ actors: [] }));
		const search = new ActorSearch({ logger: silentLogger, provider: 'bluesky', fetch, now: () => now });

		await search.search('ne');
		now = 10 * 60_000;
		await search.search('ne');
		expect(fetch).toHaveBeenCalledTimes(2);

		for (let i = 0; i < 500; i++) await search.search(`term${i}`);
		expect(fetch).toHaveBeenCalledTimes(502);
		await search.search('term499');
		expect(fetch).toHaveBeenCalledTimes(502);
		await search.search('ne');
		expect(fetch).toHaveBeenCalledTimes(503);
	});

	it('finds nothing for a letter, with the search off, or when Bluesky fails', async () => {
		const fetch = vi.fn<typeof globalThis.fetch>(async () => new Response('down', { status: 502 }));
		const warn = vi.spyOn(silentLogger, 'warn');

		expect(await new ActorSearch({ logger: silentLogger, provider: 'bluesky', fetch }).search('n')).toEqual([]);
		expect(await new ActorSearch({ logger: silentLogger, provider: 'none', fetch }).search('neon')).toEqual([]);
		expect(fetch).not.toHaveBeenCalled();

		expect(await new ActorSearch({ logger: silentLogger, provider: 'bluesky', fetch }).search('neon')).toEqual([]);
		expect(warn).toHaveBeenCalledWith(expect.objectContaining({ term: 'neon' }), 'actor typeahead failed');
		warn.mockRestore();
	});
});
