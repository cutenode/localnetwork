/**
 * A recording request's `Range` header through parseRange, which anyone can send: whatever the header, a range it
 * returns lies inside the file. Each header is tried against a few file sizes, including either side of each number in
 * it, where off-by-one mistakes show.
 */
import { RECORDING_MAX_BYTES } from '../src/lib/server/audio.ts';
import { parseRange } from '../src/lib/server/media/audio.ts';

export const seeds = ['fuzz/seeds/range'];

export function fuzz(data: Buffer): void {
	const header = data.toString('latin1');
	const numbers = (header.match(/\d+/g) ?? []).map(Number).flatMap((n) => [n, n + 1]);

	// Recordings are 1 byte to RECORDING_MAX_BYTES (services/recordings.ts).
	for (const size of [1, 2, 1024, RECORDING_MAX_BYTES, ...numbers]) {
		if (!(size >= 1 && size <= RECORDING_MAX_BYTES)) continue;
		const range = parseRange(header, size);

		if (range === null || range === 'unsatisfiable') continue;
		const { start, end } = range;

		if (!(Number.isSafeInteger(start) && Number.isSafeInteger(end) && start >= 0 && start <= end && end < size))
			throw new Error(`parseRange gives ${start}-${end} of ${size} bytes`);
	}
}
