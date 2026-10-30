import { once } from 'node:events';
import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { TID } from '@atproto/common-web';
import { isRedirect } from '@sveltejs/kit';
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';
import { DEFAULT_ACCESS, type EpkContent, type EpkFields, type EpkRecord } from '../../src/lib/epk.ts';
import { COLLECTION, EPK_CONTENT_KEY, SPACE_TYPE } from '../../src/lib/nsid.ts';
import { validateRecord } from '../../src/lib/server/atproto/lexicons.ts';
import { silentLogger } from '../../src/lib/server/logger.ts';
import { actions as askActions } from '../../src/routes/dashboard/access/[slug]/[rkey]/+page.server.ts';
import { actions as newKit } from '../../src/routes/dashboard/projects/[slug]/press-kits/new/+page.server.ts';
import {
	actions as editorActions,
	load as editorLoad
} from '../../src/routes/dashboard/projects/[slug]/press-kits/[rkey]/+page.server.ts';
import {
	actions as accessActions,
	load as accessLoad
} from '../../src/routes/dashboard/projects/[slug]/press-kits/[rkey]/access/+page.server.ts';
import { GET as oldKit } from '../../src/routes/epk/[did]/[rkey]/+server.ts';
import { GET as oldPhoto } from '../../src/routes/epk/[did]/[rkey]/photos/[cid]/+server.ts';
import { GET as oldList } from '../../src/routes/projects/[slug]/epk/+server.ts';
import { load as publicKit } from '../../src/routes/projects/[slug]/press-kits/[rkey]/+page.server.ts';
import type { PressKit } from '../../src/lib/server/services/epk.ts';
import type { ProjectView } from '../../src/lib/types.ts';
import { TestNetwork } from './network.ts';

/** A 1x1 PNG. */
const PNG = Uint8Array.from(
	atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='),
	(char) => char.charCodeAt(0)
);

const kit = (overrides: EpkFields = {}): EpkFields => ({
	title: 'Clubs',
	headline: 'Late rooms',
	access: DEFAULT_ACCESS,
	content: { bio: 'The long bio', techRider: '2x CDJ-3000' },
	...overrides
});

describe('shared press kits on a real atproto spaces network', () => {
	let net: TestNetwork;

	beforeAll(async () => {
		net = await TestNetwork.create(['ada', 'neon', 'remy', 'pat'], { admins: ['ada'] });
	});
	afterAll(async () => {
		await net?.close();
	});

	const project = async (name: string, managers: string[]): Promise<ProjectView> => {
		const ada = await net.viewer('ada');

		const created = await net.app.projectAdmin.create({
			actor: ada,
			profile: { name, kind: 'duo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } }
		});

		return net.app.projectAdmin.setManagers({
			actor: ada,
			project: created,
			managers: managers.map((manager) => net.user(manager).did)
		});
	};

	/** A press kit as a manager made one before the root held them: the record in their repo, the rest in their space. */
	const legacyKit = async (
		name: string,
		owned: ProjectView,
		parts: { record: Omit<EpkRecord, 'project' | 'access' | 'createdAt'>; content: Omit<EpkContent, 'updatedAt'> }
	) => {
		const owner = net.user(name).client;
		const rkey = TID.nextStr();
		const now = new Date().toISOString();

		const space = await owner.createSpace({
			spaceType: SPACE_TYPE.epk,
			skey: rkey,
			readPolicy: { $type: 'com.atproto.simplespace.defs#memberListPolicy' },
			writePolicy: { $type: 'com.atproto.simplespace.defs#publicPolicy' },
			appAccess: { $type: 'com.atproto.simplespace.defs#open' }
		});

		await owner.putMember({ space, did: net.rootDid, read: true, write: true });
		await owner.putSpaceRecord({
			space,
			collection: COLLECTION.epkContent,
			rkey: EPK_CONTENT_KEY,
			record: validateRecord(COLLECTION.epkContent, { ...parts.content, updatedAt: now })
		});

		const { uri } = await owner.putRecord({
			collection: COLLECTION.epk,
			rkey,
			record: validateRecord(COLLECTION.epk, {
				...parts.record,
				project: owned.uri,
				access: DEFAULT_ACCESS,
				createdAt: now
			})
		});

		// The app reads press kits from HappyView, which hears of this one over Jetstream.
		await expect
			.poll(async () => (await net.app.happyView.query('getRecords', { uris: [uri] })).records.length, {
				timeout: 10_000
			})
			.toBe(1);

		return { rkey, space, now };
	};

	const members = async (space: string) =>
		Object.fromEntries(
			(await (await net.app.core.root()).listMembers(space)).map(({ did, ...access }) => [did, access])
		);

	it('lets two managers draft, publish and share one press kit, which the root holds', async () => {
		const [neon, remy, pat] = [await net.viewer('neon'), await net.viewer('remy'), net.user('pat').did];
		let tide = await project('Neon Tide', ['neon', 'remy']);
		const { drafts } = net.app.epks;
		const photo = { image: await drafts.uploadPhoto(neon, PNG, 'image/png'), alt: 'Neon Tide' };
		const draft = await drafts.save(neon, tide, { values: kit({ photos: [photo] }) });
		const first = await drafts.publish(neon, tide, draft!.rkey);

		if (first.kind !== 'published') throw new Error('the first draft conflicts');
		const { epk } = first;
		const root = await net.app.core.root();

		expect((await root.getRecord({ collection: COLLECTION.epk, rkey: epk.rkey }))?.value).toMatchObject({
			project: tide.uri,
			title: 'Clubs',
			photos: [{ alt: 'Neon Tide' }]
		});
		// The photo now lives in the root's repo, so it outlasts its uploader leaving the project.
		expect((await net.app.core.fetchBlob!(net.rootDid, photo.image.ref.$link, 1_000_000)).bytes).toEqual(PNG);
		expect(await members(drafts.space(tide))).toEqual({
			[neon.did]: { read: true, write: true },
			[remy.did]: { read: true, write: true }
		});
		expect(await members(epk.space)).toEqual({
			[neon.did]: { read: true, write: false },
			[remy.did]: { read: true, write: false }
		});

		// Both open the press kit as published, so each draft saves over that version.
		await drafts.save(remy, tide, {
			rkey: epk.rkey,
			cid: epk.cid,
			values: kit({ photos: [photo], headline: "Remy's headline" })
		});
		await drafts.save(neon, tide, {
			rkey: epk.rkey,
			cid: epk.cid,
			values: kit({
				photos: [photo],
				headline: "Neon's headline",
				content: { bio: 'A newer bio', techRider: '2x CDJ-3000' }
			})
		});
		expect((await drafts.publish(remy, tide, epk.rkey)).kind).toBe('published');
		expect((await net.app.epks.dashboard(tide)).kits[0].drafts.map((entry) => entry.status)).toEqual(['behind']);

		const conflict = await drafts.publish(neon, tide, epk.rkey);
		expect(conflict).toMatchObject({
			kind: 'conflict',
			conflicts: [{ field: 'headline', published: "Remy's headline" }]
		});
		const last = await drafts.publish(neon, tide, epk.rkey, { overwrite: ['headline'] });
		expect(last).toMatchObject({ kind: 'published', epk: { record: { headline: "Neon's headline" } } });
		expect(await net.app.epks.content(epk)).toMatchObject({ bio: 'A newer bio', techRider: '2x CDJ-3000' });

		// The manager who didn't make it shares it, and the root writes the grant.
		const shared: PressKit = { epk, project: tide };
		await net.app.epks.invite(remy, shared, 'pat.test');
		expect(await members(epk.space)).toMatchObject({ [pat]: { read: true, write: false } });

		const grant = await root.getSpaceRecord({
			space: epk.space,
			repo: net.rootDid,
			collection: COLLECTION.epkGrant,
			rkey: pat
		});

		expect(grant?.value).toMatchObject({ subject: pat, via: 'invited' });
		expect(await net.app.epks.accessState(shared, await net.viewer('pat'))).toMatchObject({ kind: 'granted' });

		tide = await net.app.projectAdmin.setManagers({
			actor: await net.viewer('ada'),
			project: tide,
			managers: [neon.did]
		});
		expect(await members(drafts.space(tide))).toEqual({ [neon.did]: { read: true, write: true } });
		expect(await members(epk.space)).toEqual({
			[neon.did]: { read: true, write: false },
			[pat]: { read: true, write: false }
		});
	});

	it("won't follow a redirect from the PDS an account's DID document names when fetching its blob", async () => {
		// A PDS that sends getBlob somewhere else, as one could to reach an address it couldn't name itself.
		const pds = createServer((request, response) => {
			if (request.url?.startsWith('/xrpc/com.atproto.sync.getBlob')) response.writeHead(302, { location: '/inside' });
			response.end(PNG);
		});

		await once(pds.listen(0, '127.0.0.1'), 'listening');
		// SAFETY: a server listening on a TCP port reports an AddressInfo.
		const { port } = pds.address() as AddressInfo;
		const did = 'did:plc:redirects';
		const resolve = net.app.identity.resolve.bind(net.app.identity);
		const redirecting = { did, handle: null, pds: `http://127.0.0.1:${port}`, signingKey: '', kemKey: null };

		const spy = vi
			.spyOn(net.app.identity, 'resolve')
			.mockImplementation(async (asked, forceRefresh) => (asked === did ? redirecting : resolve(asked, forceRefresh)));

		try {
			await expect(net.app.core.fetchBlob!(did, 'bafkreigh2akiscaildc', 1_000_000)).rejects.toMatchObject({
				cause: { message: 'unexpected redirect' }
			});
		} finally {
			spy.mockRestore();
			await new Promise((done) => pds.close(done));
		}
	});

	it("moves a manager's legacy press kit to the root on their next visit, with its photos and grants", async () => {
		const neon = await net.viewer('neon');
		const pat = net.user('pat').did;
		const halflight = await project('Halflight', ['neon', 'remy']);
		const owner = net.user('neon').client;

		const [wide, secret] = [
			await owner.uploadBlob(PNG, 'image/png'),
			await owner.uploadBlob(Uint8Array.of(...PNG, 0), 'image/png')
		];

		const { rkey, space, now } = await legacyKit('neon', halflight, {
			record: { title: 'Old clubs', photos: [{ image: wide, alt: 'Halflight' }] },
			content: { bio: 'The long bio', photos: [{ image: secret, alt: 'Backstage' }] }
		});

		await owner.putMember({ space, did: pat, read: true, write: false });
		await owner.putSpaceRecord({
			space,
			collection: COLLECTION.epkGrant,
			rkey: pat,
			record: validateRecord(COLLECTION.epkGrant, { subject: pat, via: 'invited', createdAt: now })
		});

		expect(await net.app.epks.migrateLegacy(neon)).toEqual({ migrated: [rkey], failed: [] });

		const root = await net.app.core.root();
		const [moved] = await net.app.epks.forProject(halflight);
		expect(moved).toMatchObject({
			did: net.rootDid,
			rkey,
			legacy: false,
			record: { title: 'Old clubs', createdAt: now }
		});
		expect(await net.app.epks.content(moved)).toMatchObject({ bio: 'The long bio', photos: [{ alt: 'Backstage' }] });
		expect((await net.app.core.fetchBlob!(net.rootDid, wide.ref.$link, 1_000_000)).bytes).toEqual(PNG);
		expect(
			(await root.getSpaceRecord({ space: moved.space, repo: net.rootDid, collection: COLLECTION.epkGrant, rkey: pat }))
				?.value
		).toMatchObject({ subject: pat, via: 'invited' });
		expect(await members(moved.space)).toEqual({
			[neon.did]: { read: true, write: false },
			[net.user('remy').did]: { read: true, write: false },
			[pat]: { read: true, write: false }
		});
		expect(await owner.getSpace(space)).toBeNull();
		expect(await owner.getRecord({ collection: COLLECTION.epk, rkey })).toBeNull();

		expect(await net.app.epks.migrateLegacy(neon)).toEqual({ migrated: [], failed: [] });
	});

	it('serves the press kit pages: old links, the editor, conflicts, and access for any manager', async () => {
		const [neon, remy, pat] = [await net.viewer('neon'), await net.viewer('remy'), await net.viewer('pat')];
		const night = await project('Night Shift', ['neon', 'remy']);
		const slug = night.slug;
		const locals = (viewer: typeof neon | null) => ({ app: net.app, viewer, log: silentLogger });

		/** A handler's result as a promise, so a redirect it throws, even synchronously, rejects it. */
		const handled = async <T>(run: () => T | Promise<T>): Promise<T> => run();

		const post = (path: string, viewer: typeof neon, fields: [string, string][], params: Record<string, string>) => {
			const url = new URL(`http://127.0.0.1${path}`);
			const body = new FormData();

			for (const [name, value] of fields) body.append(name, value);

			return { params, url, request: new Request(url, { method: 'POST', body }), locals: locals(viewer) };
		};

		const kitForm = (headline: string, bio: string, cid = ''): [string, string][] => [
			['cid', cid],
			['title', 'Night Shift'],
			['headline', headline],
			['bio', bio],
			['preset', 'review']
		];

		const created = await handled(() =>
			newKit.publish(post(`/dashboard/projects/${slug}/press-kits/new`, neon, kitForm('First', 'First bio'), { slug }))
		).catch((cause: unknown) => cause);

		if (!isRedirect(created)) throw new Error('publishing a new press kit should redirect to it');
		expect(created).toMatchObject({ status: 303, location: expect.stringContaining('?notice=published') });
		const rkey = /press-kits\/([^/?]+)/.exec(created.location)![1];
		const page = `/dashboard/projects/${slug}/press-kits/${rkey}`;
		const params = { slug, rkey };

		const editing = (viewer: typeof neon) =>
			editorLoad({ params, url: new URL(`http://x${page}`), locals: locals(viewer) });

		/** The editor's form as `viewer` opens it now, filled in: it carries the published version it shows. */
		const opened = async (viewer: typeof neon, headline: string, bio: string) =>
			kitForm(headline, bio, (await editing(viewer)).values.cid);

		// Links pasted before the move keep working.
		await expect(
			handled(() => oldKit({ params: { did: neon.did, rkey }, url: new URL('http://x/'), locals: locals(null) }))
		).rejects.toMatchObject({
			status: 308,
			location: `/projects/${slug}/press-kits/${rkey}`
		});
		await expect(
			handled(() =>
				oldPhoto({ params: { did: net.rootDid, rkey, cid: 'bafy' }, url: new URL('http://x/'), locals: locals(null) })
			)
		).rejects.toMatchObject({ status: 308, location: `/projects/${slug}/press-kits/${rkey}/photos/bafy` });
		await expect(handled(() => oldList({ params: { slug }, url: new URL('http://x/?print=1') }))).rejects.toMatchObject(
			{
				status: 308,
				location: `/projects/${slug}/press-kits?print=1`
			}
		);

		// Remy saves a draft, then drops it.
		const remyForm = await opened(remy, 'Remy', 'Remy bio');
		await expect(handled(() => editorActions.save(post(page, remy, remyForm, params)))).rejects.toMatchObject({
			status: 303,
			location: `${page}?notice=saved`
		});

		expect(await editing(remy)).toMatchObject({ draft: { status: 'current' }, values: { headline: 'Remy' } });
		expect(await editing(neon)).toMatchObject({ draft: null, others: [{ author: { did: remy.did } }] });
		await expect(handled(() => editorActions.discard(post(page, remy, [], params)))).rejects.toMatchObject({
			location: `${page}?notice=discarded`
		});
		expect(await editing(remy)).toMatchObject({ draft: null, values: { headline: 'First' } });

		// Both change the headline and bio; Neon publishes first, so Remy's draft is behind.
		const before = await opened(remy, 'Remy', 'Remy bio');
		await expect(handled(() => editorActions.save(post(page, remy, before, params)))).rejects.toMatchObject({
			location: `${page}?notice=saved`
		});
		const neonForm = await opened(neon, 'Neon', 'Neon bio');
		await expect(handled(() => editorActions.publish(post(page, neon, neonForm, params)))).rejects.toMatchObject({
			location: `${page}?notice=published`
		});
		expect(await editing(remy)).toMatchObject({ draft: { status: 'behind' } });

		// The form Remy still has open shows the version before Neon's, so saving it would take Neon's back: reload.
		expect(await editorActions.publish(post(page, remy, before, params))).toMatchObject({ status: 409 });

		// Reloaded, publishing asks about both fields.
		const conflict = await editorActions.publish(post(page, remy, await opened(remy, 'Remy', 'Remy bio'), params));
		expect(conflict).toMatchObject({
			conflicts: [
				{ field: 'headline', published: 'Neon', previous: 'First', draft: 'Remy' },
				{ field: 'bio', published: 'Neon bio', previous: 'First bio', draft: 'Remy bio' }
			]
		});

		// Remy keeps their headline and Neon's bio.
		const resolved = [
			['conflict', 'headline'],
			['conflict', 'bio'],
			['overwrite', 'headline']
		] satisfies [string, string][];

		await expect(handled(() => editorActions.resolve(post(page, remy, resolved, params)))).rejects.toMatchObject({
			location: `${page}?notice=published`
		});
		const root = await net.app.core.root();
		expect((await root.getRecord({ collection: COLLECTION.epk, rkey }))?.value).toMatchObject({
			headline: 'Remy',
			bio: 'Neon bio'
		});

		// Pat asks; Remy, who didn't make it, lets them in; then the press kit shows them the rest.
		const asked = await askActions.request(
			post(`/dashboard/access/${slug}/${rkey}`, pat, [['message', 'For a booking']], params)
		);

		expect(asked).toMatchObject({ requested: 'pending' });
		const access = await accessLoad({ params, url: new URL(`http://x${page}/access`), locals: locals(remy) });
		const [request] = access.requests;
		expect(request).toMatchObject({ requester: { did: pat.did }, message: 'For a booking' });
		expect(await accessActions.approve(post(`${page}/access`, remy, [['request', request.uri]], params))).toMatchObject(
			{
				success: expect.stringContaining('Approved')
			}
		);

		const shown = (viewer: typeof neon | null) => publicKit({ params, locals: locals(viewer) });
		expect(await shown(pat)).toMatchObject({ access: { kind: 'granted' }, content: { updatedAt: expect.any(String) } });
		expect(await shown(null)).toMatchObject({ access: { kind: 'signedOut' }, content: null });

		// A co-manager opening a press kit that hasn't moved yet is told who has to open it.
		const { rkey: old } = await legacyKit('remy', night, {
			record: { title: 'Old nights' },
			content: { bio: 'Old bio' }
		});
		// The app reads managers' repos for legacy press kits at most once a minute.

		net.app.epks.forget(remy.did);
		expect(
			await editorLoad({ params: { slug, rkey: old }, url: new URL('http://x/'), locals: locals(neon) })
		).toMatchObject({
			legacyMaker: { did: remy.did }
		});
	});
});
