import { afterEach, beforeEach, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { ResolvedIdentity } from '#lib/server/atproto/identity.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { RateLimiter } from '#lib/server/rate-limit.ts';
import { DjService } from '#lib/server/services/dj.ts';
import { createTestServices } from '#lib/server/testing/core.ts';
import { callRoute, routeEvent } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

const MARA = 'did:plc:mara';

const OSKAR = 'did:plc:oskar';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices();

	// Mara has a profile here; Oskar has only his account.
	await t.net.client(MARA).putRecord({
		collection: COLLECTION.profile,
		rkey: 'self',
		record: {
			$type: COLLECTION.profile,
			basics: { name: 'Mara Lind', location: { city: 'Leipzig', country: 'DE' } },
			createdAt: '2026-10-30T00:00:00.000Z',
			updatedAt: '2026-10-30T00:00:00.000Z'
		}
	});
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** Accounts' handles, verified both ways, as the identity resolver reports them. */
const handles = new Map([
	[MARA, 'mara.test'],
	[OSKAR, 'oskar.test']
]);

/** An identity resolver over `handles`, with the PDS every account here is on. */
const identity = {
	resolve: async (did: string): Promise<ResolvedIdentity> => {
		if (!handles.has(did)) throw new Error(`could not resolve ${did}`);

		return { did, handle: handles.get(did) ?? null, pds: 'https://pds.test', signingKey: 'did:key:z', kemKey: null };
	},
	resolveHandle: async (handle: string) => [...handles].find(([, known]) => known === handle)?.[0] ?? null
};

/** The app, with the site's public URL, the identity resolver, DJs' own records, and room for `misses` misses. */
const app = (misses = 30) => ({
	...t,
	config: { publicUrl: 'https://localnetwork.test' },
	identity,
	dj: new DjService(t.indexedRepo, silentLogger),
	sets: t.projects.sets,
	limits: { profileMisses: new RateLimiter(misses, misses / 600) }
});

it("shows a person's profile at their handle, with structured data for search engines", async () => {
	const { loadData, locals } = routeEvent({ app: app(), params: { handle: 'mara.test' }, url: '/@mara.test' });
	const data = await loadData<PageServerData>(load);

	expect(data).toMatchObject({ name: 'Mara Lind', account: { did: MARA, handle: 'mara.test' }, sets: undefined });
	expect(data.meta).toMatchObject({
		title: 'Mara Lind',
		description: 'Mara Lind, Leipzig, DE',
		url: 'https://localnetwork.test/@mara.test'
	});
	expect(await data.playsIn).toEqual([]);
	expect(locals).toMatchObject({ crawlerHead: expect.stringContaining('application/ld+json') });
});

it('shows an account without a profile by its handle, without structured data', async () => {
	const { loadData, locals } = routeEvent({ app: app(), params: { handle: 'oskar.test' }, url: '/@oskar.test' });
	const data = await loadData<PageServerData>(load);

	expect(data).toMatchObject({ profile: null, name: '@oskar.test' });
	expect(locals).toMatchObject({ crawlerHead: expect.not.stringContaining('application/ld+json') });
});

it('sends a DID or a handle written another way to the canonical address, keeping the query', async () => {
	await expect(
		callRoute(load, { app: app(), params: { handle: MARA }, url: `/@${MARA}?tab=sets` })
	).rejects.toMatchObject({ status: 302, location: '/@mara.test?tab=sets' });
	await expect(
		callRoute(load, { app: app(), params: { handle: 'Mara.Test' }, url: '/@Mara.Test' })
	).rejects.toMatchObject({ status: 302, location: '/@mara.test' });
});

it('is a 404 for an address that names no account, and a 429 once a visitor has tried too many', async () => {
	const limited = app(1);

	await expect(
		callRoute(load, { app: limited, params: { handle: 'nobody.test' }, url: '/@nobody.test' })
	).rejects.toMatchObject({ status: 404 });
	await expect(
		callRoute(load, { app: limited, params: { handle: 'mara.test' }, url: '/@mara.test' })
	).rejects.toMatchObject({ status: 429 });
});
