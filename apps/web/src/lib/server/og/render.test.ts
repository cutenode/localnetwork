import { imageSize } from 'image-size';
import sharp from 'sharp';
import { afterEach, describe, expect, it, vi } from 'vitest';
import ArtistCard from '#lib/og/cards/ArtistCard.svelte';
import { fitPng, MAX_CARD_BYTES, renderCard } from './render';

/** A 1200×630 PNG of random pixels, which no palette shrinks much: the worst a photo behind a card could be. */
const noise = async () =>
	new Uint8Array(
		await sharp({
			create: {
				width: 1200,
				height: 630,
				channels: 3,
				background: '#808080',
				noise: { type: 'gaussian', mean: 128, sigma: 90 }
			}
		})
			.png()
			.toBuffer()
	);

describe('drawing a card', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
	});

	it('asks nothing of the network, leaving out characters its fonts lack', async () => {
		vi.stubGlobal('fetch', async (url: string) => {
			throw new Error(`a card fetched ${url}`);
		});

		const artist = { name: 'Ferrule', djCount: 2, tracks: [] };
		const png = await renderCard(ArtistCard, { ...artist, name: 'Ferrule 🌊 夜 ❤️' });

		expect(imageSize(png)).toMatchObject({ type: 'png', width: 1200, height: 630 });
		// Drawn as if the emoji and the 夜 weren't there, rather than as empty boxes.
		expect(png).toEqual(await renderCard(ArtistCard, artist));
	});

	it('re-encodes a card over the limit with fewer colours until it fits', async () => {
		const png = await noise();

		expect(png.byteLength).toBeGreaterThan(MAX_CARD_BYTES);
		const fitted = await fitPng(png);

		expect(fitted.byteLength).toBeLessThanOrEqual(MAX_CARD_BYTES);
		expect(imageSize(fitted)).toMatchObject({ type: 'png', width: 1200, height: 630 });
	});

	it('fails rather than send a card no palette brings under the limit', async () => {
		await expect(fitPng(await noise(), 10_000)).rejects.toThrow('over 10000 bytes');
	});
});
