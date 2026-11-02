import { afterEach, describe, expect, it, vi } from 'vitest';
import { searchActors } from './actors.ts';

const neon = { did: 'did:plc:neon', handle: 'neon.test', displayName: 'Neon Tide' };

/** Stands in for the server: `/api/actors?q=neon tide` answers with `body` and `status`, anything else is a 404. */
function serve(body: string, status = 200) {
	vi.stubGlobal('fetch', async (url: string) =>
		url === '/api/actors?q=neon%20tide' ? new Response(body, { status }) : new Response(null, { status: 404 })
	);
}

afterEach(() => vi.unstubAllGlobals());

describe('searchActors', () => {
	it('returns the accounts the server found', async () => {
		serve(JSON.stringify({ actors: [neon] }));
		expect(await searchActors('neon tide')).toEqual([neon]);
	});

	it('finds nothing when the server refuses the search', async () => {
		serve('Too many requests', 429);
		expect(await searchActors('neon tide')).toEqual([]);
	});
});
