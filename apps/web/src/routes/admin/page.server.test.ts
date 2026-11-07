import { afterEach, beforeEach, expect, it } from 'vitest';
import { createTestServices, ROOT } from '#lib/server/testing/core.ts';
import { loadRoute } from '#lib/testing/route-event.ts';
import type { PageServerData } from './$types';
import { load } from './+page.server.ts';

const ADMIN = 'did:plc:admin';

const DJ = 'did:plc:dj';

let t: Awaited<ReturnType<typeof createTestServices>>;

beforeEach(async () => {
	t = await createTestServices({ admins: [ADMIN], capable: [DJ] });
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/** The app with a syncer watching the audit space cleanly and failing to read the staff space. */
const app = () => ({
	...t,
	syncer: {
		status: async () => [
			{ space: t.spaces.audit, lastSyncedAt: '2026-11-01T10:00:00.000Z', lastError: null },
			{ space: t.spaces.staff, lastSyncedAt: null, lastError: 'fetch failed' }
		]
	}
});

it('sums up the queue, the directory, the audit log, and sync for staff', async () => {
	const request = await t.requests.submit({
		actor: await t.viewer(DJ),
		kind: 'create',
		profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } }
	});

	await t.audit.idle();

	const data = await loadRoute<PageServerData>(load, { app: app(), viewer: await t.viewer(ADMIN), url: '/admin' });

	expect(data.requests.pending).toBe(1);
	expect(data.pending.map((item) => item.uri)).toEqual([request.uri]);
	expect(data.projects).toMatchObject({ total: 0 });
	expect(data.audit.total).toBe(6);
	expect(data.recent[0]).toMatchObject({ action: 'request.submit', actor: { did: DJ, handle: null } });
	expect(data.recent.at(-1)).toMatchObject({ action: 'system.space', actor: { did: ROOT } });
	expect({ syncErrors: data.syncErrors, spaces: data.spaces }).toEqual({ syncErrors: 1, spaces: 2 });
});

it('sends someone signed out to sign in', async () => {
	await expect(loadRoute<PageServerData>(load, { app: app(), url: '/admin' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fadmin'
	});
});
