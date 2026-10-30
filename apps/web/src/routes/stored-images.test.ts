import { type HttpError, isHttpError, isRedirect, type Redirect } from '@sveltejs/kit';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { BlobJson } from '#lib/server/atproto/repo-client.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { ROOT } from '#lib/server/testing/core.ts';
import { DJ, pressKitServices } from '#lib/server/testing/press-kits.ts';
import { GET as editorPhoto } from './dashboard/projects/[slug]/press-kits/[rkey]/photos/[cid]/+server.ts';
import { GET as requestAvatar } from './dashboard/requests/[rkey]/avatar/+server.ts';
import { GET as oldPhoto } from './epk/[did]/[rkey]/photos/[cid]/+server.ts';
import { GET as media } from './media/[did]/[cid]/+server.ts';
import { GET as photo } from './projects/[slug]/press-kits/[rkey]/photos/[cid]/+server.ts';

const PDS = 'https://pds.test';

/** What a route answers: a response, or the HTTP error or redirect it throws. */
type Outcome = Response | HttpError | Redirect;

describe('routes serving stored images', () => {
	let f: Awaited<ReturnType<typeof pressKitServices>>;

	beforeEach(async () => {
		f = await pressKitServices();
	});
	afterEach(async () => {
		await f.t.audit.idle();
		await f.t.db.destroy();
	});

	/** The root's PDS answering `com.atproto.sync.getBlob` with each blob as the type it was uploaded as. */
	const pds = async (url: URL) => {
		const blob = f.t.net.blobs.get(url.searchParams.get('cid') ?? '');

		return blob
			? new Response(new Uint8Array(blob.bytes), { headers: { 'content-type': blob.mimeType } })
			: new Response(null, { status: 404 });
	};

	/** What `handler` answers a GET of `path` with, as `viewer`. */
	const get = async (
		handler: (event: never) => Response | Promise<Response>,
		path: string,
		params: Record<string, string>,
		viewer: string | null
	): Promise<Outcome> => {
		const { t } = f;
		const app = { ...t, rootDid: ROOT, identity: { resolve: async () => ({ pds: PDS }) } };
		const locals = { app, viewer: viewer && (await t.viewer(viewer)), log: silentLogger };
		const event = { url: new URL(path, 'https://x.test'), params, locals, fetch: pds };

		try {
			// SAFETY: these routes read only the URL, params, locals and fetch, and the services are the app's own.
			return await handler(event as never);
		} catch (cause) {
			if (isHttpError(cause) || isRedirect(cause)) return cause;
			throw cause;
		}
	};

	/** The response a route answered with, failing the test on an error it threw instead. */
	const served = (outcome: Outcome): Response => {
		if (outcome instanceof Response) return outcome;
		throw new Error(`expected a response, got ${outcome.status}`);
	};

	/** A press kit published with `image` as its one public photo. */
	const kitWith = async (image: BlobJson) => {
		const kit = await f.publish(DJ, { photos: [{ image, alt: 'Live at the warehouse' }] });

		return { slug: f.project.slug, rkey: kit.rkey, cid: image.ref.$link };
	};

	/** Each route, given a blob of some type to serve, and the query string to ask for it with. */
	const routes: [string, (bytes: Uint8Array, mimeType: string, query: string) => Promise<Outcome>][] = [
		[
			'a press kit photo',
			async (bytes, mimeType, query) => {
				const params = await kitWith(await f.t.net.client(DJ).uploadBlob(bytes, mimeType));

				return get(
					photo,
					`/projects/${params.slug}/press-kits/${params.rkey}/photos/${params.cid}${query}`,
					params,
					null
				);
			}
		],
		[
			'a photo in the press kit editor',
			async (bytes, mimeType, query) => {
				const params = await kitWith(await f.t.net.client(DJ).uploadBlob(bytes, mimeType));
				const path = `/dashboard/projects/${params.slug}/press-kits/${params.rkey}/photos/${params.cid}${query}`;

				return get(editorPhoto, path, params, DJ);
			}
		],
		[
			'a project avatar',
			async (bytes, mimeType, query) => {
				const { ref } = await f.t.net.client(ROOT).uploadBlob(bytes, mimeType);

				return get(media, `/media/${ROOT}/${ref.$link}${query}`, { did: ROOT, cid: ref.$link }, null);
			}
		],
		[
			"a requested project's avatar",
			async (bytes, mimeType, query) => {
				const request = await f.t.requests.submit({
					actor: await f.t.viewer(DJ),
					kind: 'create',
					profile: { name: 'Glow', kind: 'solo', genres: ['house'], location: { city: 'Berlin', country: 'DE' } },
					avatar: { bytes, mimeType }
				});

				return get(requestAvatar, `/dashboard/requests/${request.rkey}/avatar${query}`, { rkey: request.rkey }, DJ);
			}
		]
	];

	describe.each(routes)('%s', (_name, serve) => {
		it.each(['image/png', 'image/jpeg', 'image/webp', 'IMAGE/PNG'])(
			'serves %s inline, sandboxed and unsniffed',
			async (mimeType) => {
				const bytes = new TextEncoder().encode(`a ${mimeType} image`);
				const response = served(await serve(bytes, mimeType, ''));

				expect(response.status).toBe(200);
				expect(Object.fromEntries(response.headers)).toMatchObject({
					'content-type': mimeType.toLowerCase(),
					'content-disposition': 'inline',
					'content-security-policy': "default-src 'none'; sandbox",
					'x-content-type-options': 'nosniff'
				});
				expect(new Uint8Array(await response.arrayBuffer())).toEqual(bytes);
			}
		);

		it.each(['image/svg+xml', 'image/x-unknown', 'text/html'])('refuses %s with a 404', async (mimeType) => {
			const svg = '<svg xmlns="http://www.w3.org/2000/svg"><script>alert(document.domain)</script></svg>';

			expect(await serve(new TextEncoder().encode(svg), mimeType, '')).toMatchObject({ status: 404 });
		});
	});

	// Press kits offer their photos as downloads; avatars have no download link.
	describe.each(routes.slice(0, 2))('%s', (_name, serve) => {
		it('sends it as an attachment with ?download', async () => {
			const response = served(await serve(new TextEncoder().encode('a photo'), 'image/jpeg', '?download'));

			expect(response.headers.get('content-disposition')).toBe('attachment');
		});
	});

	it('keeps ?download on an old photo link it sends on', async () => {
		const params = await kitWith(await f.t.net.client(DJ).uploadBlob(new TextEncoder().encode('a photo'), 'image/png'));

		expect(
			await get(oldPhoto, `/epk/${ROOT}/${params.rkey}/photos/${params.cid}?download`, { did: ROOT, ...params }, null)
		).toMatchObject({
			status: 308,
			location: `/projects/${params.slug}/press-kits/${params.rkey}/photos/${params.cid}?download`
		});
	});
});
