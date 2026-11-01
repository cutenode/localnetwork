import { type HttpError, isHttpError, isRedirect, type Redirect } from '@sveltejs/kit';
import { imageSize } from 'image-size';
import sharp from 'sharp';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { MixReport } from '#lib/mix/report.ts';
import { sampleReport } from '#lib/mix/sample.ts';
import { COLLECTION } from '#lib/nsid.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { HappyViewError } from '#lib/server/happyview/client.ts';
import { MAX_CARD_BYTES } from '#lib/server/og/render.ts';
import type { SupportService } from '#lib/server/services/supports.ts';
import { ROOT } from '#lib/server/testing/core.ts';
import { ADMIN, DJ, pressKitServices } from '#lib/server/testing/press-kits.ts';
import type { ProjectView } from '#lib/types.ts';
import { GET as artistCard } from './artist/[mbid].png/+server.ts';
import { GET as mixCard } from './mix/[slug]/[n]/[cid].png/+server.ts';
import { GET as pressKitCard } from './press-kit/[slug]/[rkey]/[cid].png/+server.ts';
import { GET as projectCard } from './project/[slug]/[cid].png/+server.ts';
import { GET as siteCard } from './site.png/+server.ts';

/** What a route answers: a response, or the HTTP error or redirect it throws. */
type Outcome = Response | HttpError | Redirect;

const MBID = '2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41';

/** A noisy photo, the hardest kind of picture to keep a PNG of small. */
const photo = (width: number, height: number) =>
	sharp({
		create: { width, height, channels: 3, background: '#808080', noise: { type: 'gaussian', mean: 128, sigma: 60 } }
	})
		.jpeg()
		.toBuffer();

describe('link-preview cards', () => {
	let f: Awaited<ReturnType<typeof pressKitServices>>;
	let project: ProjectView;

	beforeEach(async () => {
		f = await pressKitServices();
		const cover = `data:image/jpeg;base64,${(await photo(200, 200)).toString('base64')}`;

		const json = (report: MixReport) => ({
			bytes: new TextEncoder().encode(JSON.stringify(report)),
			mimeType: 'application/json'
		});

		// The first mix's report has no cover, so its card shows the performer's picture; the second's has one.
		project = await f.t.projectAdmin.update({
			actor: await f.t.viewer(ADMIN),
			project: f.project,
			profile: {
				...f.project.profile,
				mixes: [
					{ title: 'Late Room', url: 'https://mixes.test/late-room' },
					{ title: 'Warehouse', url: 'https://mixes.test/warehouse' },
					{ title: 'No tracklist', url: 'https://mixes.test/plain' }
				]
			},
			avatar: { bytes: await photo(400, 400), mimeType: 'image/jpeg' },
			tracklists: [json(sampleReport), json({ ...sampleReport, mix: { ...sampleReport.mix, cover } })]
		});
	});
	afterEach(async () => {
		await f.t.audit.idle();
		await f.t.db.destroy();
	});

	/** What `handler` answers an anonymous GET with, as a crawler asks, with any of the app's services replaced. */
	const get = async (
		handler: (event: never) => Response | Promise<Response>,
		params: Record<string, string>,
		services: { supports?: Pick<SupportService, 'artist'> } = {}
	): Promise<Outcome> => {
		const locals = { app: { ...f.t, rootDid: ROOT, ...services }, viewer: null, log: silentLogger };

		try {
			// SAFETY: these routes read only params and locals, and the services are the app's own.
			return await handler({ params, locals } as never);
		} catch (cause) {
			if (isHttpError(cause) || isRedirect(cause)) return cause;
			throw cause;
		}
	};

	/** The card a route answered with, checked to be a 1200×630 PNG small enough for every app's preview. */
	const card = async (outcome: Outcome) => {
		if (!(outcome instanceof Response)) throw new Error(`expected a card, got ${outcome.status}`);
		const png = new Uint8Array(await outcome.arrayBuffer());

		expect(outcome.headers.get('content-type')).toBe('image/png');
		expect(imageSize(png)).toMatchObject({ type: 'png', width: 1200, height: 630 });
		expect(png.byteLength).toBeLessThanOrEqual(MAX_CARD_BYTES);

		return outcome.headers.get('cache-control');
	};

	const IMMUTABLE = 'public, max-age=31536000, immutable';

	describe("a performer's", () => {
		const params = (cid: string) => ({ slug: project.slug, cid });

		it('is cached for good at its CID, and sends an old CID to the current card', async () => {
			expect(await card(await get(projectCard, params(project.cid)))).toBe(IMMUTABLE);
			expect(await get(projectCard, params('bafyold'))).toMatchObject({
				status: 302,
				location: `/og/project/${project.slug}/${project.cid}.png`
			});
		});

		it.each([
			['not a picture', 'not a picture'],
			// sharp would read an SVG, whatever type it was stored as.
			['an SVG stored as a PNG', '<svg xmlns="http://www.w3.org/2000/svg" width="9" height="9"/>']
		])('shows the monogram when the avatar is %s', async (_name, text) => {
			const broken = await f.t.projectAdmin.update({
				actor: await f.t.viewer(ADMIN),
				project,
				profile: project.profile,
				avatar: { bytes: new TextEncoder().encode(text), mimeType: 'image/png' }
			});

			expect(await card(await get(projectCard, params(broken.cid)))).toBe(IMMUTABLE);
		});

		it("is a 404 while the project isn't active, as its page is to anyone signed out", async () => {
			const hidden = await f.t.projectAdmin.setStatus({
				actor: await f.t.viewer(ADMIN),
				project,
				status: 'hidden'
			});

			expect(await get(projectCard, params(hidden.cid))).toMatchObject({ status: 404 });
			expect(await get(projectCard, { slug: 'nobody', cid: hidden.cid })).toMatchObject({ status: 404 });
		});
	});

	describe("a mix's", () => {
		it('is drawn from its tracklist and cached for good at the project CID', async () => {
			expect(await card(await get(mixCard, { slug: project.slug, n: '1', cid: project.cid }))).toBe(IMMUTABLE);
			expect(await card(await get(mixCard, { slug: project.slug, n: '2', cid: project.cid }))).toBe(IMMUTABLE);
			expect(await get(mixCard, { slug: project.slug, n: '1', cid: 'bafyold' })).toMatchObject({
				status: 302,
				location: `/og/mix/${project.slug}/1/${project.cid}.png`
			});
		});

		it("is a 404 for a mix that isn't there or has no tracklist", async () => {
			for (const n of ['3', '4', '0', 'x']) {
				expect(await get(mixCard, { slug: project.slug, n, cid: project.cid })).toMatchObject({ status: 404 });
			}
		});
	});

	describe("a press kit's", () => {
		it('shows its first public photo, cached for good at the press kit CID', async () => {
			const image = await f.t.net.client(DJ).uploadBlob(await photo(2400, 1600), 'image/jpeg');
			const kit = await f.publish(DJ, { photos: [{ image, alt: 'Neon Tide behind the decks' }] });
			const params = (cid: string) => ({ slug: project.slug, rkey: kit.rkey, cid });

			expect(await card(await get(pressKitCard, params(kit.cid)))).toBe(IMMUTABLE);
			expect(await get(pressKitCard, params(project.cid))).toMatchObject({
				status: 302,
				location: `/og/press-kit/${project.slug}/${kit.rkey}/${kit.cid}.png`
			});
		});

		it("is the performer card without a photo, and a 404 for a kit that isn't there", async () => {
			const kit = await f.publish(DJ);

			expect(await card(await get(pressKitCard, { slug: project.slug, rkey: kit.rkey, cid: kit.cid }))).toBe(IMMUTABLE);
			expect(await get(pressKitCard, { slug: project.slug, rkey: '3kxxxxxxxxx22', cid: kit.cid })).toMatchObject({
				status: 404
			});
		});
	});

	describe("an artist's", () => {
		/** A set that credits the artist `mbid`. */
		const credit = (mbid: string) =>
			f.t.net.client(DJ).putRecord({
				collection: COLLECTION.djSet,
				rkey: '3kset1',
				record: {
					$type: COLLECTION.djSet,
					date: '2026-09-12',
					kind: 'club',
					venue: 'Halle Nord',
					location: {
						$type: 'community.lexicon.location.address',
						name: 'Halle Nord',
						locality: 'Berlin',
						country: 'DE'
					},
					tracks: [
						{ title: 'Night Bus', artist: 'Ferrule', artists: [{ artistName: 'Ferrule', artistMbId: `mbid:${mbid}` }] }
					],
					createdAt: '2026-10-01T00:00:00Z'
				}
			});

		const offline = {
			supports: {
				artist: async () => {
					throw new HappyViewError(503, 'offline');
				}
			}
		};

		it('counts the DJs who played them, kept for an hour, and served from memory without the directory', async () => {
			await credit(MBID);

			expect(await card(await get(artistCard, { mbid: MBID }))).toBe('public, max-age=3600');
			expect(await card(await get(artistCard, { mbid: MBID }, offline))).toBe('public, max-age=3600');
		});

		it('asks a crawler to come back while the directory is offline', async () => {
			const answer = await get(artistCard, { mbid: '0b9f7c1e-2d3a-4e5f-8a6b-7c8d9e0f1a2b' }, offline);

			expect(answer).toMatchObject({ status: 503 });
			expect(answer instanceof Response && answer.headers.get('retry-after')).toBe('60');
		});

		it('is a 404 for an artist no set credits, or an address that is no MusicBrainz ID', async () => {
			expect(await get(artistCard, { mbid: '5e6f7a8b-9c0d-4e1f-a2b3-c4d5e6f7a8b9' })).toMatchObject({ status: 404 });
			expect(await get(artistCard, { mbid: 'ferrule' })).toMatchObject({ status: 404 });
		});
	});

	it("the site's is kept for a day", async () => {
		expect(await card(await get(siteCard, {}))).toBe('public, max-age=86400');
	});
});
