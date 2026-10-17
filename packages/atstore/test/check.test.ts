import { describe, expect, it } from 'vitest';
import { fetchRules, LISTING, problems, rulesOf, type Asset, type Lexicon } from '../src/check.ts';

const IMAGES = ['image/png', 'image/jpeg', 'image/webp', 'image/gif', 'image/svg+xml'];

/** The listing lexicon as the store publishes it, its picture fields at least, with `fields` laid over them. */
type Properties = Extract<Lexicon['defs'][string], { type: 'record' }>['record']['properties'];

function listing(fields: Properties = {}): Lexicon {
	return {
		lexicon: 1,
		id: LISTING,
		defs: {
			main: {
				type: 'record',
				key: 'tid',
				record: {
					type: 'object',
					required: ['icon'],
					properties: {
						icon: { type: 'blob', accept: IMAGES, maxSize: 2_000_000 },
						heroImage: { type: 'blob', accept: IMAGES, maxSize: 12_000_000 },
						screenshots: { type: 'array', maxLength: 20, items: { type: 'blob', accept: IMAGES, maxSize: 12_000_000 } },
						...fields
					}
				}
			}
		}
	};
}

const rules = rulesOf(listing());

/** The start of a PNG `width` by `height`, which is all a size check reads, padded to `bytes`. */
function png(width: number, height: number, bytes = 64): Buffer {
	const head = Buffer.alloc(Math.max(bytes, 33));
	Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]).copy(head);
	head.writeUInt32BE(13, 8);
	head.write('IHDR', 12, 'ascii');
	head.writeUInt32BE(width, 16);
	head.writeUInt32BE(height, 20);
	head[24] = 8;
	head[25] = 6;

	return head;
}

const icon: Asset = { file: 'icon.png', field: 'icon', png: png(1024, 1024) };

const hero: Asset = { file: 'hero.png', field: 'heroImage', png: png(1920, 1080) };

const screenshot = (n: number): Asset => ({ file: `screenshot-${n}.png`, field: 'screenshots', png: png(800, 1200) });

describe('problems', () => {
	it('finds none in an icon, a hero and four screenshots of the right shapes', () => {
		expect(problems(rules, [icon, hero, ...[1, 2, 3, 4].map(screenshot)])).toEqual([]);
	});

	it('wants an icon', () => {
		expect(problems(rules, [hero])).toEqual(['no icon, which a listing needs']);
	});

	it('takes four screenshots, as many as the listing form does', () => {
		expect(problems(rules, [icon, ...[1, 2, 3, 4, 5].map(screenshot)])).toEqual([
			'5 screenshots, more than the 4 a listing takes'
		]);
	});

	it('wants a square icon and a 16:9 hero, which the form would crop', () => {
		expect(
			problems(rules, [
				{ ...icon, png: png(1024, 1000) },
				{ ...hero, png: png(1920, 1200) }
			])
		).toEqual(["icon.png: 1024x1000 isn't 1:1", "hero.png: 1920x1200 isn't 16:9"]);
	});

	it('holds each picture to the size its field takes', () => {
		expect(problems(rules, [{ ...icon, png: png(1024, 1024, 2_000_001) }])).toEqual([
			'icon.png: 2000001 bytes, over the 2000000 it takes'
		]);
		expect(problems(rules, [icon, { ...screenshot(1), png: png(800, 1200, 12_000_000) }])).toEqual([]);
	});

	it('refuses a kind of image the record does not take', () => {
		const bmp = Buffer.alloc(64);
		bmp.write('BM', 0, 'ascii');
		bmp.writeUInt32LE(40, 14);
		bmp.writeInt32LE(1024, 18);
		bmp.writeInt32LE(1024, 22);
		expect(problems(rules, [{ ...icon, png: bmp }])).toEqual([
			"icon.png: image/bmp isn't one of image/png, image/jpeg, image/webp, image/gif, image/svg+xml"
		]);
	});
});

describe('rulesOf', () => {
	it('reads the types and size limit of each picture field, and how many screenshots a record holds', () => {
		expect(rules).toEqual({
			icon: { accept: IMAGES, maxSize: 2_000_000 },
			heroImage: { accept: IMAGES, maxSize: 12_000_000 },
			screenshots: { accept: IMAGES, maxSize: 12_000_000, maxLength: 20 }
		});
	});

	it('says what changed shape when the store changes its lexicon', () => {
		expect(() => rulesOf(listing({ icon: { type: 'string', format: 'uri' } }))).toThrow(
			"fyi.atstore.listing.detail's icon is no longer a blob with types and a size limit"
		);
		expect(() => rulesOf(listing({ heroImage: { type: 'blob', accept: IMAGES } }))).toThrow(
			"fyi.atstore.listing.detail's heroImage is no longer a blob with types and a size limit"
		);
		expect(() => rulesOf(listing({ screenshots: { type: 'array', items: { type: 'blob' } } }))).toThrow(
			"fyi.atstore.listing.detail's screenshots are no longer a list with a length limit"
		);
		expect(() => rulesOf({ lexicon: 1, id: LISTING, defs: { main: { type: 'token' } } })).toThrow(
			'fyi.atstore.listing.detail is no longer a record'
		);
	});
});

describe('fetchRules', () => {
	it('asks the resolver for the listing lexicon and reads its rules', async () => {
		const asked: string[] = [];

		const resolver = {
			get: async (nsid: string) => {
				asked.push(nsid);

				return { lexicon: listing() };
			}
		};

		expect(await fetchRules(resolver)).toEqual(rules);
		expect(asked).toEqual([LISTING]);
	});
});
