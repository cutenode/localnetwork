import { isHttpError, isRedirect } from '@sveltejs/kit';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import { silentLogger } from './logger';
import { loadProfilePage } from './profile-page';
import { DjService } from './services/dj';
import { createTestServices, ROOT } from './testing/core';

const ADA = 'did:plc:ada';

const BEA = 'did:plc:bea';

/** An account without a verified handle. */
const CY = 'did:plc:cy';

/** Each DID document's verified handle (null for none), and where each handle points, as identity resolution says. */
const HANDLES = new Map<string, string | null>([
	[ADA, 'ada.test'],
	[BEA, 'bea.test'],
	[CY, null]
]);

const RESOLVES = new Map([
	['ada.test', ADA],
	['bea.test', BEA],
	// Points at Ada, whose DID document names another handle.
	['old-ada.test', ADA],
	// Points at Cy, whose DID document doesn't name it back.
	['cy.test', CY]
]);

describe('loadProfilePage', () => {
	let t: Awaited<ReturnType<typeof createTestServices>>;

	beforeEach(async () => {
		t = await createTestServices();

		for (const [did, name] of [
			[ADA, 'Ada'],
			[CY, 'Cy']
		])
			await t.net.client(did).putRecord({
				collection: COLLECTION.profile,
				rkey: 'self',
				record: {
					$type: COLLECTION.profile,
					basics: { name },
					performer: {},
					createdAt: '2026-10-30T00:00:00.000Z',
					updatedAt: '2026-10-30T00:00:00.000Z'
				}
			});
	});
	afterEach(async () => {
		await t.audit.idle();
		await t.db.destroy();
	});

	const identity = {
		resolve: async (did: string) => {
			if (!HANDLES.has(did)) throw new Error(`Could not resolve ${did}`);

			return { did, handle: HANDLES.get(did) ?? null };
		},
		resolveHandle: async (handle: string) => RESOLVES.get(handle) ?? null
	};

	/** What loading `/@<param>` does: the page's data, or the redirect or HTTP error it throws. */
	const open = async (param: string, search = '') => {
		const app = {
			...t,
			identity,
			dj: new DjService(t.indexedRepo, silentLogger),
			config: { publicUrl: 'https://localnetwork.test' }
		};

		try {
			// SAFETY: the loader reads only these services, the viewer and the logger from locals.
			return await loadProfilePage({ app, viewer: null, log: silentLogger } as never, param, search);
		} catch (cause) {
			if (isRedirect(cause)) return { redirect: cause.location, status: cause.status };

			if (isHttpError(cause)) return { status: cause.status };
			throw cause;
		}
	};

	it('shows a profile at its handle, with the active projects its DJ plays in', async () => {
		const profile = { name: 'Ada Crew', kind: 'duo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } };
		await t.projects.publish({ profile, managers: [ADA], status: 'active', createdBy: ROOT });
		await t.projects.publish({
			profile: { ...profile, name: 'Old Crew' },
			managers: [ADA],
			status: 'archived',
			createdBy: ROOT
		});
		const page = await open('ada.test');

		expect(page).toMatchObject({
			account: { did: ADA, handle: 'ada.test' },
			canonical: 'https://localnetwork.test/@ada.test',
			profile: { did: ADA, uri: `at://${ADA}/${COLLECTION.profile}/self`, record: { basics: { name: 'Ada' } } }
		});
		expect('playsIn' in page && (await page.playsIn)?.map((project) => project.profile.name)).toEqual(['Ada Crew']);
	});

	it('sends a DID, or a handle in another form, to the handle its DID document names', async () => {
		expect(await open(ADA, '?from=x')).toEqual({ status: 302, redirect: '/@ada.test?from=x' });
		expect(await open('Ada.TEST')).toEqual({ status: 302, redirect: '/@ada.test' });
		expect(await open('old-ada.test')).toEqual({ status: 302, redirect: '/@ada.test' });
	});

	it('shows an account without a verified handle at its DID only', async () => {
		expect(await open(CY)).toMatchObject({
			account: { did: CY, handle: null },
			canonical: `https://localnetwork.test/@${CY}`
		});
		expect(await open('cy.test')).toEqual({ status: 404 });
	});

	it("is a 404 for an account without a profile, or one that isn't there", async () => {
		expect(await open('bea.test')).toEqual({ status: 404 });
		expect(await open('nobody.test')).toEqual({ status: 404 });
		expect(await open('did:plc:nobody')).toEqual({ status: 404 });
		expect(await open('not a handle')).toEqual({ status: 404 });
		expect(await open('did:nonsense')).toEqual({ status: 404 });
	});
});
