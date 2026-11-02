import { isHttpError, isRedirect } from '@sveltejs/kit';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { BSKY_PROFILE, COLLECTION } from '#lib/nsid.ts';
import { silentLogger } from './logger';
import { loadProfilePage } from './profile-page';
import { RateLimiter } from './rate-limit';
import { DjService } from './services/dj';
import { createTestServices, ROOT } from './testing/core';

const ADA = 'did:plc:ada';

const BEA = 'did:plc:bea';

/** An account without a verified handle. */
const CY = 'did:plc:cy';

/** An account with neither a profile nor a Bluesky name the directory keeps. */
const DEE = 'did:plc:dee';

/** Each DID document's verified handle (null for none), and where each handle points, as identity resolution says. */
const HANDLES = new Map<string, string | null>([
	[ADA, 'ada.test'],
	[BEA, 'bea.test'],
	[CY, null],
	[DEE, 'dee.test']
]);

const RESOLVES = new Map([
	['ada.test', ADA],
	['bea.test', BEA],
	['dee.test', DEE],
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
		resolveHandle: async (handle: string) => {
			lookups.push(handle);
			// A lookup takes a moment, as one over the network does, so requests made together overlap.
			await new Promise((resolve) => setTimeout(resolve, 5));

			return RESOLVES.get(handle) ?? null;
		}
	};

	/** The handles the resolver was asked for, in order. */
	let lookups: string[] = [];

	beforeEach(() => {
		lookups = [];
	});

	/** The app's limit on lookups that find no account, fresh for each test. */
	let profileMisses: RateLimiter;

	beforeEach(() => {
		profileMisses = new RateLimiter(4, 4 / 600);
	});

	/** What loading `/@<param>` does, from `ip`: the page's data, or the redirect or HTTP error it throws. */
	const open = async (param: string, search = '', ip = '203.0.113.1') => {
		const app = {
			...t,
			identity,
			dj: new DjService(t.indexedRepo, silentLogger),
			config: { publicUrl: 'https://localnetwork.test' },
			limits: { profileMisses }
		};

		try {
			// SAFETY: the loader reads only these services, the viewer and the logger from locals.
			return await loadProfilePage({ app, viewer: null, log: silentLogger } as never, param, search, ip);
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
			account: { did: CY, handle: null }
		});
		expect(await open('cy.test')).toEqual({ status: 404 });
	});

	it('gives an account without a profile a page with its Bluesky name, or else its handle', async () => {
		const profile = { name: 'Bea Crew', kind: 'duo', genres: ['house'], location: { city: 'Leipzig', country: 'DE' } };
		await t.projects.publish({ profile, managers: [BEA], status: 'active', createdBy: ROOT });
		// The directory keeps Bluesky display names of accounts that manage a project.
		await t.net.client(BEA).putRecord({
			collection: BSKY_PROFILE,
			rkey: 'self',
			record: { $type: BSKY_PROFILE, displayName: 'Bea' }
		});

		const bea = await open('bea.test');
		expect(bea).toMatchObject({ profile: null, name: 'Bea', account: { did: BEA, handle: 'bea.test' } });
		expect('playsIn' in bea && (await bea.playsIn)?.map((project) => project.profile.name)).toEqual(['Bea Crew']);
		expect(await open('dee.test')).toMatchObject({ profile: null, name: '@dee.test' });
	});

	it('is a 404 for an address that names no account', async () => {
		expect(await open('nobody.test')).toEqual({ status: 404 });
		expect(await open('did:plc:nobody')).toEqual({ status: 404 });
		expect(await open('not a handle')).toEqual({ status: 404 });
		expect(await open('did:nonsense')).toEqual({ status: 404 });
	});

	it('charges each lookup before it starts, so a burst of misses from one visitor reaches the resolver at most the limit times', async () => {
		const names = Array.from({ length: 10 }, (_, i) => `nobody-${i}.test`);
		const pages = await Promise.all(names.map((name) => open(name)));

		expect(lookups).toHaveLength(4);
		expect(pages.filter((page) => 'status' in page && page.status === 429)).toHaveLength(6);
	});

	it('gives back what a lookup that finds an account was charged', async () => {
		for (let i = 0; i < 10; i++) expect(await open('ada.test')).toMatchObject({ account: { did: ADA } });
		expect(await open('Ada.TEST')).toEqual({ status: 302, redirect: '/@ada.test' });
		expect(await open('nobody.test')).toEqual({ status: 404 });
	});

	it('stops looking names up for a visitor after a few that find no account', async () => {
		for (const name of ['nobody.test', 'nobody-else.test', 'did:plc:nobody', 'did:nonsense'])
			expect(await open(name)).toEqual({ status: 404 });

		// Even a real account, since telling them apart would mean resolving it.
		expect(await open('ada.test')).toEqual({ status: 429 });
		expect(await open('ada.test', '', '203.0.113.2')).toMatchObject({ account: { did: ADA } });
	});
});
