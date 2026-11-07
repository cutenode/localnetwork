import { once } from 'node:events';
import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { TID } from '@atproto/common-web';
import { isRedirect } from '@sveltejs/kit';
import { afterAll, beforeAll, describe, expect, it, vi } from 'vitest';
import { z } from 'zod';
import { DEFAULT_ACCESS, type EpkContent, type EpkFields, type EpkRecord } from '../../src/lib/epk.ts';
import { COLLECTION, EPK_CONTENT_KEY, SPACE_TYPE } from '../../src/lib/nsid.ts';
import { validateRecord } from '../../src/lib/server/atproto/lexicons.ts';
import { SpaceCredentialManager, XrpcResponseError } from '../../src/lib/server/atproto/credentials.ts';
import { AgentRootClient, memberSpacePolicy } from '../../src/lib/server/atproto/repo-client.ts';
import { epkRequestSpace } from '../../src/lib/server/atproto/spaces.ts';
import { silentLogger } from '../../src/lib/server/logger.ts';
import { actions as askActions } from '../../src/routes/dashboard/access/[slug]/[rkey]/+page.server.ts';
import { actions as newKit } from '../../src/routes/dashboard/projects/[slug]/press-kits/new/+page.server.ts';
import {
	actions as editorActions,
	load as editorLoad
} from '../../src/routes/dashboard/projects/[slug]/press-kits/[rkey]/+page.server.ts';
import {
	actions as kitsActions,
	load as kitsLoad
} from '../../src/routes/dashboard/projects/[slug]/press-kits/+page.server.ts';
import {
	actions as accessActions,
	load as accessLoad
} from '../../src/routes/dashboard/projects/[slug]/press-kits/[rkey]/access/+page.server.ts';
import { GET as oldKit } from '../../src/routes/epk/[did]/[rkey]/+server.ts';
import { GET as oldPhoto } from '../../src/routes/epk/[did]/[rkey]/photos/[cid]/+server.ts';
import { GET as oldList } from '../../src/routes/projects/[slug]/epk/+server.ts';
import { load as publicKit } from '../../src/routes/projects/[slug]/press-kits/[rkey]/+page.server.ts';
import { grantKey, type PressKit } from '../../src/lib/server/services/epk.ts';
import type { ProjectView } from '../../src/lib/types.ts';
import { TestNetwork } from './network.ts';

/** A 1x1 PNG. */
const PNG = Uint8Array.from(
	atob('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='),
	(char) => char.charCodeAt(0)
);

const listReposOutput = z.object({ repos: z.array(z.object({ did: z.string() })) });

const listRecordsOutput = z.object({ records: z.array(z.object({ value: z.record(z.string(), z.unknown()) })) });

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
		net = await TestNetwork.create(['ada', 'neon', 'remy', 'pat', 'lou'], { admins: ['ada'] });
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
			policy: memberSpacePolicy({ publicWrite: true })
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

	/**
	 * Every record of `collection` (or of any) that `name` can read in `space`, in anyone's repo, with a space credential
	 * minted for their own account, as any atproto client of theirs would get one.
	 */
	const readable = async (name: string, space: string, collection?: string) => {
		const credentials = new SpaceCredentialManager({
			source: async () => new AgentRootClient(net.user(name).did, net.user(name).agent),
			identity: net.app.identity,
			logger: silentLogger
		});

		const read = <T>(audience: string, nsid: string, params: Record<string, string>, schema: z.ZodType<T>) =>
			credentials.query({ space, service: net.pds.url, audience, nsid, params: { space, ...params }, schema });

		const { repos } = await read(net.rootDid, 'com.atproto.space.listRepos', {}, listReposOutput);

		const pages = await Promise.all(
			repos.map(({ did }) =>
				read(did, 'com.atproto.space.listRecords', { repo: did, ...(collection && { collection }) }, listRecordsOutput)
			)
		);

		return pages.flatMap((page) => page.records.map((record) => record.value));
	};

	/** Who the grant records `name` can read in `space` let in. */
	const grantees = async (name: string, space: string) =>
		(await readable(name, space, COLLECTION.epkGrant)).map((value) => String(value.subject)).sort();

	/** A press kit `name` publishes for `owned`. */
	const published = async (name: string, owned: ProjectView): Promise<PressKit> => {
		const maker = await net.viewer(name);
		const draft = await net.app.epks.drafts.save(maker, owned, { values: kit() });
		const result = await net.app.epks.drafts.publish(maker, owned, draft!.rkey);

		if (result.kind !== 'published') throw new Error('the draft conflicts');

		return { epk: result.epk, project: owned };
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
			space: epkRequestSpace(net.rootDid, tide.rkey),
			repo: net.rootDid,
			collection: COLLECTION.epkGrant,
			rkey: grantKey(epk.rkey, pat)
		});

		expect(grant?.value).toMatchObject({ subject: pat, epk: epk.uri, via: 'invited' });
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

	it('keeps every request from the people let into the press kit, and from everyone else who asks', async () => {
		const [neon, pat, lou] = [await net.viewer('neon'), await net.viewer('pat'), await net.viewer('lou')];
		const shared = await published('neon', await project('Low Light', ['neon']));
		const requests = epkRequestSpace(net.rootDid, shared.project.rkey);

		await net.app.epks.request(pat, shared, { message: 'Pat asking' });
		const [asked] = await net.app.epks.requests(shared.epk);
		await net.app.epks.decide(neon, shared, asked.uri, 'approve');
		await net.app.epks.request(lou, shared, { role: 'promoter', message: "Lou's rates, just for the managers" });
		await net.app.epks.invite(neon, shared, 'remy.test');

		// Pat, let in, reads the private part with their own credential, and nothing else in its space: no request, Lou's
		// or their own, and no grant, Remy's or their own.
		expect(await readable('pat', shared.epk.space)).toMatchObject([
			{ $type: COLLECTION.epkContent, bio: 'The long bio' }
		]);
		expect(await readable('pat', shared.epk.space, COLLECTION.epkGrant)).toEqual([]);
		// Nor can they, or Lou, read the space the requests are in: only the project's managers can.
		await expect(readable('pat', requests, COLLECTION.epkRequest)).rejects.toBeInstanceOf(XrpcResponseError);
		await expect(readable('lou', requests, COLLECTION.epkRequest)).rejects.toBeInstanceOf(XrpcResponseError);
		expect(
			(await readable('neon', requests, COLLECTION.epkRequest)).map((value) => String(value.message)).sort()
		).toEqual(["Lou's rates, just for the managers", 'Pat asking'].sort());
		// They read who's let in there too, which the people let in can't.
		expect(
			(await readable('neon', requests, COLLECTION.epkGrant))
				.map((value) => `${String(value.subject)} ${String(value.via)}`)
				.sort()
		).toEqual([`${pat.did} approved`, `${net.user('remy').did} invited`].sort());
		await expect(readable('pat', requests, COLLECTION.epkGrant)).rejects.toBeInstanceOf(XrpcResponseError);
		expect(await members(requests)).toEqual({ [neon.did]: { read: true, write: false } });

		// Lou's PDS still takes a request written straight into the press kit's space, but its authority no longer counts
		// a writer who isn't a member, so nobody let in finds it there.
		await net.user('lou').client.createSpaceRecord({
			space: shared.epk.space,
			collection: COLLECTION.epkRequest,
			record: validateRecord(COLLECTION.epkRequest, { epk: shared.epk.uri, createdAt: new Date().toISOString() })
		});
		await net.pds.processAll();
		expect(await readable('pat', shared.epk.space, COLLECTION.epkRequest)).toEqual([]);
	});

	it("moves requests out of a press kit's space, where everyone let in could read them, at startup", async () => {
		const [neon, lou] = [await net.viewer('neon'), net.user('lou').did];
		const shared = await published('neon', await project('Old Rooms', ['neon']));
		const { epk } = shared;
		const root = await net.app.core.root();

		// As press kit spaces were: anyone wrote a request into one, and everyone let in read it.
		await root.updateSpace({ space: epk.space, policy: memberSpacePolicy({ publicWrite: true }) });
		await net.user('lou').client.createSpaceRecord({
			space: epk.space,
			collection: COLLECTION.epkRequest,
			record: validateRecord(COLLECTION.epkRequest, {
				epk: epk.uri,
				message: 'An old note',
				createdAt: new Date().toISOString()
			})
		});
		await net.app.epks.invite(neon, shared, 'pat.test');
		await net.settle();
		expect(await net.app.epks.requests(epk)).toMatchObject([{ requester: { did: lou }, message: 'An old note' }]);
		expect((await readable('pat', epk.space, COLLECTION.epkRequest)).map((value) => value.message)).toEqual([
			'An old note'
		]);

		await net.app.bootstrap();

		expect(await readable('pat', epk.space, COLLECTION.epkRequest)).toEqual([]);
		const requests = epkRequestSpace(net.rootDid, shared.project.rkey);
		expect(await readable('neon', requests, COLLECTION.epkRequest)).toMatchObject([
			{ epk: epk.uri, message: 'An old note', onBehalfOf: lou }
		]);
		expect(await net.app.epks.requests(epk)).toMatchObject([
			{ uri: expect.stringContaining(requests), requester: { did: lou }, message: 'An old note', status: 'pending' }
		]);
		expect((await root.getSpace(epk.space))?.policy).toMatchObject({ publicWrite: false });
	});

	it("moves grants out of a press kit's space, where everyone let in could list them, at startup", async () => {
		const neon = await net.viewer('neon');
		const [pat, lou] = [net.user('pat').did, net.user('lou').did];
		const shared = await published('neon', await project('Old Doors', ['neon']));
		const { epk } = shared;
		const root = await net.app.core.root();

		// As grants were: the root's record of each person let in sat in the press kit's space, beside the private part.
		for (const subject of [pat, lou]) {
			await root.putMember({ space: epk.space, did: subject, read: true, write: false });
			await root.putSpaceRecord({
				space: epk.space,
				collection: COLLECTION.epkGrant,
				rkey: subject,
				record: validateRecord(COLLECTION.epkGrant, { subject, via: 'invited', createdAt: new Date().toISOString() })
			});
		}

		await net.settle();
		expect(await grantees('pat', epk.space)).toEqual([pat, lou].sort());

		await net.app.bootstrap();

		expect(await readable('pat', epk.space)).toMatchObject([{ $type: COLLECTION.epkContent }]);
		const requests = epkRequestSpace(net.rootDid, shared.project.rkey);
		expect(await grantees('neon', requests)).toEqual([pat, lou].sort());
		expect(await members(epk.space)).toEqual({
			[neon.did]: { read: true, write: false },
			[pat]: { read: true, write: false },
			[lou]: { read: true, write: false }
		});

		await net.settle();
		expect((await net.app.epks.grants(epk)).map((grant) => grant.subject.did).sort()).toEqual([pat, lou].sort());
		expect(await net.app.epks.accessState(shared, await net.viewer('lou'))).toMatchObject({ kind: 'granted' });

		// Any manager still takes access away.
		await net.app.epks.revoke(neon, shared, lou);
		expect(await grantees('neon', requests)).toEqual([pat]);
		expect(await members(epk.space)).not.toHaveProperty(lou);
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

		const grant = await root.getSpaceRecord({
			space: epkRequestSpace(net.rootDid, halflight.rkey),
			repo: net.rootDid,
			collection: COLLECTION.epkGrant,
			rkey: grantKey(rkey, pat)
		});

		expect(grant?.value).toMatchObject({ subject: pat, epk: moved.uri, via: 'invited' });
		expect(await readable('pat', moved.space)).toMatchObject([{ $type: COLLECTION.epkContent }]);
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
		expect(await shown(pat)).toMatchObject({
			access: { kind: 'granted' },
			content: { updatedAt: expect.any(String) },
			details: {}
		});
		expect(await shown(null)).toMatchObject({ access: { kind: 'signedOut' }, content: null, details: null });

		// Pat's look leaves the managers a receipt; theirs don't. Either manager can delete it, and the chain still holds.
		await shown(remy);
		const kits = `/dashboard/projects/${slug}/press-kits`;

		const receipts = async (viewer: typeof neon) =>
			(
				await kitsLoad({
					url: new URL(`http://x${kits}`),
					locals: locals(viewer),
					parent: async () => ({ project: night })
				})
			).views;

		const [receipt, ...others] = await receipts(remy);
		expect(others).toEqual([]);
		expect(receipt).toMatchObject({ viewer: { did: pat.did }, epk: { rkey, title: 'Night Shift' } });
		expect(await kitsActions.forgetView(post(kits, neon, [['view', receipt.id]], { slug }))).toMatchObject({
			success: 'View deleted.'
		});
		expect(await receipts(remy)).toEqual([]);
		expect(await net.app.audit.verifyChain()).toMatchObject({ ok: true });

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
