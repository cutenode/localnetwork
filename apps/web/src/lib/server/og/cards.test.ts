import sharp from 'sharp';
import { describe, expect, it } from 'vitest';
import { PHOTO_TYPES } from '../images';
import { silentLogger } from '../logger';
import { cardImage } from './cards';

const size = { width: 120, height: 63, accept: PHOTO_TYPES };

const picture = (width: number, height: number) =>
	sharp({ create: { width, height, channels: 3, background: '#3987e5' } });

describe("a card's picture", () => {
	it('is a JPEG data URL at the size the card draws it, whatever type it was', async () => {
		const webp = await picture(400, 300).webp().toBuffer();
		const drawn = await cardImage(webp, size, silentLogger);

		expect(drawn).toMatch(/^data:image\/jpeg;base64,/);
		const { width, height } = await sharp(Buffer.from(drawn!.split(',')[1], 'base64')).metadata();

		expect({ width, height }).toEqual({ width: 120, height: 63 });
	});

	it("is left out when it's a type the lexicon doesn't accept, whatever it was stored as", async () => {
		const svg = new TextEncoder().encode('<svg xmlns="http://www.w3.org/2000/svg" width="400" height="300"/>');

		expect(await cardImage(svg, size, silentLogger)).toBeNull();
		expect(await cardImage(await picture(40, 30).tiff().toBuffer(), size, silentLogger)).toBeNull();
		expect(await cardImage(new TextEncoder().encode('not a picture'), size, silentLogger)).toBeNull();
	});

	it('is left out over 40 megapixels, before it is decoded', async () => {
		const huge = await picture(6400, 6400).png({ compressionLevel: 9 }).toBuffer();

		expect(await cardImage(huge, size, silentLogger)).toBeNull();
		expect(await cardImage(await picture(6000, 6000).png().toBuffer(), size, silentLogger)).not.toBeNull();
	});
});
