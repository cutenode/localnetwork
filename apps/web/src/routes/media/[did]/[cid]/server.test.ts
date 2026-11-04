import { afterEach, beforeEach, expect, it } from 'vitest';
import { COLLECTION } from '#lib/nsid.ts';
import type { ResolvedIdentity } from '#lib/server/atproto/identity.ts';
import { HappyViewClient } from '#lib/server/happyview/client.ts';
import { IndexedRepo } from '#lib/server/happyview/indexed.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { ProfileService } from '#lib/server/services/profiles.ts';
import { createTestServices, ROOT } from '#lib/server/testing/core.ts';
import { callRoute } from '#lib/testing/route-event.ts';
import { GET } from './+server.ts';

const MARA = 'did:plc:mara';

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

let t: Awaited<ReturnType<typeof createTestServices>>;

/** The CID of Mara's avatar. */
let avatar: string;

beforeEach(async () => {
	t = await createTestServices();
	const client = t.net.client(MARA);
	const image = await client.uploadBlob(PNG, 'image/png');
	avatar = image.ref.$link;

	await client.putRecord({
		collection: COLLECTION.profile,
		rkey: 'self',
		record: {
			$type: COLLECTION.profile,
			basics: { name: 'Mara Lind', avatar: image },
			createdAt: '2026-10-30T00:00:00.000Z',
			updatedAt: '2026-10-30T00:00:00.000Z'
		}
	});
});

afterEach(async () => {
	await t.audit.idle();
	await t.db.destroy();
});

/**
 * A PDS at https://pds.test serving its accounts' blobs from `getBlob` as `type`, the way a PDS does: everything the
 * fake network holds, which a test can take away.
 */
const pds =
	(type: string | null = 'image/png'): typeof fetch =>
	async (input) => {
		const url = new URL(input instanceof Request ? input.url : input);
		const blob = t.net.blobs.get(url.searchParams.get('cid') ?? '');
		// Answers the route's blob read from the PDS, which its own ast-grep-ignore allows.
		// ast-grep-ignore: no-direct-pds-read
		const isGetBlob = url.origin === 'https://pds.test' && url.pathname === '/xrpc/com.atproto.sync.getBlob';

		if (!isGetBlob || !blob) return Response.json({ error: 'BlobNotFound' }, { status: 404 });

		return new Response(new Uint8Array(blob.bytes), { headers: type ? { 'content-type': type } : {} });
	};

/** The app, with every account's PDS at https://pds.test, reached through the guarded fetch `guarded`. */
const app = (guarded: typeof fetch = pds()) => ({
	...t,
	rootDid: ROOT,
	identity: {
		resolve: async (did: string): Promise<ResolvedIdentity> => ({
			did,
			handle: null,
			pds: 'https://pds.test',
			signingKey: 'did:key:z',
			kemKey: null
		}),
		fetch: guarded
	}
});

/** What the route answers for `did`'s blob `cid`, with its own fetch reaching the PDS as `fetch`. */
const avatarOf = (did: string, cid: string, appWith = app(), fetch: typeof globalThis.fetch = pds()) =>
	callRoute(GET, { app: appWith, params: { did, cid }, fetch });

it("serves a person's avatar from their PDS, cached for good", async () => {
	const response = await avatarOf(MARA, avatar);

	expect(response.status).toBe(200);
	expect(response.headers.get('content-type')).toBe('image/png');
	expect(response.headers.get('cache-control')).toBe('public, max-age=31536000, immutable');
	expect(new Uint8Array(await response.arrayBuffer())).toEqual(PNG);
});

it("serves a project's avatar from the root's own PDS with the app's own fetch", async () => {
	const { ref } = await t.net.client(ROOT).uploadBlob(PNG, 'image/png');
	const nothing = async () => new Response(null, { status: 404 });
	const response = await avatarOf(ROOT, ref.$link, app(nothing));

	expect(new Uint8Array(await response.arrayBuffer())).toEqual(PNG);
});

it("is a 404 for what isn't a CID, a blob that isn't a person's avatar, or a person without a profile", async () => {
	// The root's PDS would answer anything, so only the check keeps an address that isn't a CID from reaching it.
	const anything = async () => new Response(PNG, { headers: { 'content-type': 'image/png' } });

	await expect(avatarOf(ROOT, 'not-a-cid', app(), anything)).rejects.toMatchObject({ status: 404 });
	await expect(avatarOf(MARA, 'not-a-cid')).rejects.toMatchObject({ status: 404 });
	// A picture of Mara's that her PDS would serve, but her profile doesn't name as her avatar.
	const flyer = await t.net.client(MARA).uploadBlob(new Uint8Array([...PNG, 1]), 'image/png');

	await expect(avatarOf(MARA, flyer.ref.$link)).rejects.toMatchObject({ status: 404 });
	await expect(avatarOf('did:plc:nobody', avatar)).rejects.toMatchObject({ status: 404 });
});

it("is a 404 while the directory can't say which avatar a person has", async () => {
	const down = new HappyViewClient({
		url: 'http://happyview.test',
		fetch: async () => new Response(null, { status: 503 })
	});

	const profiles = new ProfileService(t.core, new IndexedRepo(down, silentLogger, 0));

	await expect(avatarOf(MARA, avatar, { ...app(), profiles })).rejects.toMatchObject({ status: 404 });
});

it("is a 404 when a person's PDS can't be reached or sends the request elsewhere", async () => {
	const unreachable = async () => {
		throw new TypeError('fetch failed');
	};

	// A PDS answering with a redirect to an address of its choosing, which only a fetch that follows it would serve.
	const redirecting: typeof fetch = async (input, init) => {
		if (init?.redirect === 'error') throw new TypeError('fetch failed: unexpected redirect');

		return pds()(input, init);
	};

	await expect(avatarOf(MARA, avatar, app(unreachable))).rejects.toMatchObject({ status: 404 });
	await expect(avatarOf(MARA, avatar, app(redirecting))).rejects.toMatchObject({ status: 404 });
});

it('says the image is unavailable when the PDS lacks it, fails, or answers with nothing', async () => {
	const failing = async () => new Response('Internal Server Error', { status: 500 });
	const empty = async () => new Response(null, { status: 200 });

	t.net.blobs.delete(avatar);

	await expect(avatarOf(MARA, avatar)).rejects.toMatchObject({ status: 404, body: { message: 'Image unavailable' } });
	await expect(avatarOf(MARA, avatar, app(failing))).rejects.toMatchObject({ status: 502 });
	await expect(avatarOf(MARA, avatar, app(empty))).rejects.toMatchObject({ status: 502 });
});

it("won't serve a blob as an avatar unless it's a type the lexicon takes", async () => {
	await expect(avatarOf(MARA, avatar, app(pds('image/svg+xml')))).rejects.toMatchObject({ status: 404 });
	await expect(avatarOf(MARA, avatar, app(pds(null)))).rejects.toMatchObject({ status: 404 });
});
