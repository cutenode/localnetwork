/**
 * An uploaded avatar or press photo, as the forms check it: sniffImage accepts the bytes for a type only when they
 * start with that type's signature, and a photo it accepts gets a size in whole pixels or none (forms/epk.ts).
 */
import { dimensions } from '../src/lib/server/forms/epk.ts';
import { sniffImage } from '../src/lib/server/forms/project.ts';

export const seeds = ['fuzz/seeds/image'];

/** Whether a file's first 12 bytes, as latin1 text, start with each type's signature. */
const signatures: [type: string, matches: (head: string) => boolean][] = [
	['image/png', (head) => head.startsWith('\x89PNG\r\n\x1a\n')],
	['image/jpeg', (head) => head.startsWith('\xff\xd8\xff')],
	// The file's length sits between RIFF and WEBP.
	['image/webp', (head) => head.startsWith('RIFF') && head.slice(8, 12) === 'WEBP'],
	// A type the forms don't take, whatever its bytes.
	['image/gif', () => false]
];

const isPixels = (n: number) => Number.isSafeInteger(n) && n > 0;

export function fuzz(data: Buffer): void {
	const bytes = new Uint8Array(data);
	const head = data.toString('latin1', 0, 12);

	for (const [type, matches] of signatures) {
		const sniffed = sniffImage(bytes, type);

		if (sniffed !== matches(head)) throw new Error(`sniffImage says ${sniffed} for ${type}`);

		if (!sniffed) continue;
		const size = dimensions(bytes);

		if (size && !(isPixels(size.width) && isPixels(size.height)))
			throw new Error(`a ${type} measures ${size.width} by ${size.height}`);
	}
}
