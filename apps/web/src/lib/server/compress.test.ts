import { constants, gunzipSync } from 'node:zlib';
import { describe, expect, it } from 'vitest';
import { gzipped } from './compress';

const page = '<!doctype html><html><body>'.padEnd(4_000, 'x');

const request = (acceptEncoding?: string) =>
	new Request('http://app.test/', { headers: acceptEncoding ? { 'accept-encoding': acceptEncoding } : {} });

const html = (body: BodyInit = page, headers: Record<string, string> = {}) =>
	new Response(body, { status: 201, headers: { 'content-type': 'text/html; charset=utf-8', ...headers } });

describe('gzipped', () => {
	it('gzips a page for a client that takes gzip', async () => {
		const response = gzipped(request('gzip, deflate, br'), html(page, { 'content-length': '4000' }));

		expect(response.status).toBe(201);
		expect(response.headers.get('content-encoding')).toBe('gzip');
		expect(response.headers.get('content-length')).toBeNull();
		expect(response.headers.get('vary')).toBe('accept-encoding');
		const body = Buffer.from(await response.arrayBuffer());
		expect(body.length).toBeLessThan(page.length / 10);
		expect(gunzipSync(body).toString()).toBe(page);
	});

	it('leaves alone what the client cannot take, binary files, and what is already encoded', () => {
		const image = new Response('png', { headers: { 'content-type': 'image/png' } });
		const encoded = html(page, { 'content-encoding': 'br' });
		const plain = html();

		expect(gzipped(request(), plain)).toBe(plain);
		expect(gzipped(request('br'), plain)).toBe(plain);
		expect(gzipped(request('gzip'), image)).toBe(image);
		expect(gzipped(request('gzip'), encoded)).toBe(encoded);
	});

	it('sends each streamed chunk while the stream is still open', async () => {
		// A page whose streamed data hasn't resolved: the first chunk is out, the stream isn't done.
		const stream = new ReadableStream<Uint8Array>({
			start: (controller) => controller.enqueue(new TextEncoder().encode('<p>first</p>'))
		});

		const reader = gzipped(request('gzip'), html(stream)).body!.getReader();
		const received: Uint8Array[] = [];

		// Without a flush per chunk, zlib holds the chunk back and this waits until the test times out.
		while (!gunzipPartial(received).includes('first')) received.push((await reader.read()).value!);
		expect(gunzipPartial(received)).toBe('<p>first</p>');
		await reader.cancel();
	});
});

/** What a partial gzip stream decodes to so far. */
function gunzipPartial(chunks: Uint8Array[]): string {
	return gunzipSync(Buffer.concat(chunks), { finishFlush: constants.Z_SYNC_FLUSH }).toString();
}
