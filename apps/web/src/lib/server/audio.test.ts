import { describe, expect, it } from 'vitest';
import { audioResponse, RECORDING_MAX_BYTES, RECORDING_TYPES } from './audio';
import { silentLogger } from './logger';

const BYTES = [1, 2, 3];

/** A body that says whether it was cancelled. */
function stream() {
	const seen = { cancelled: false };

	const body = new ReadableStream<Uint8Array>({
		start(controller) {
			controller.enqueue(Uint8Array.from(BYTES));
			controller.close();
		},
		cancel() {
			seen.cancelled = true;
		}
	});

	return { body, seen };
}

const serve = (mimeType: string, body = stream().body) =>
	audioResponse(body, mimeType, { status: 206, headers: { etag: '"cid"' }, log: silentLogger });

describe('recording types', () => {
	it("takes any audio type and video/mp4, as the lexicon's accept does", () => {
		for (const mime of ['audio/mpeg', 'audio/mp4', 'audio/ogg', 'audio/x-m4a', 'video/mp4'])
			expect(RECORDING_TYPES.matches({ mime })).toBe(true);

		for (const mime of ['video/webm', 'audiox/mpeg', 'audio/', 'audio', 'audio/*', 'text/html', 'image/svg+xml'])
			expect(RECORDING_TYPES.matches({ mime })).toBe(false);
	});

	it("caps a recording at the lexicon's maxSize, 300 MiB", () => {
		expect(RECORDING_MAX_BYTES).toBe(300 * 1024 * 1024);
	});
});

describe('audioResponse', () => {
	it("serves an audio type as it is, with the caller's status and headers, locked down", async () => {
		const response = serve('audio/mpeg');

		expect(response.status).toBe(206);
		expect([...new Uint8Array(await response.arrayBuffer())]).toEqual(BYTES);
		expect(Object.fromEntries(response.headers)).toEqual({
			etag: '"cid"',
			'content-type': 'audio/mpeg',
			'content-security-policy': "default-src 'none'; media-src 'self'; style-src 'unsafe-inline'",
			'x-content-type-options': 'nosniff'
		});
	});

	it('matches a type whatever its case, and sends it lowercased', () => {
		expect(serve('Audio/MPEG').headers.get('content-type')).toBe('audio/mpeg');
	});

	it('serves video/mp4, the type a PDS gives an M4A it reads as video, as audio/mp4', () => {
		expect(serve('video/mp4').headers.get('content-type')).toBe('audio/mp4');
		expect(serve('VIDEO/MP4').headers.get('content-type')).toBe('audio/mp4');
	});

	it("refuses a type the lexicon doesn't accept, and cancels its body", async () => {
		for (const mimeType of ['text/html', 'image/svg+xml', 'video/webm', 'application/octet-stream', 'audio/*']) {
			const { body, seen } = stream();
			const response = serve(mimeType, body);

			expect(response.status).toBe(404);
			expect(response.headers.get('content-type')).toBe('text/plain; charset=utf-8');
			expect(await response.text()).toBe('Not found');
			expect(seen.cancelled).toBe(true);
		}
	});
});
