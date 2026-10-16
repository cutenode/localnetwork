import { Duplex } from 'node:stream';
import { constants, createGzip } from 'node:zlib';

type ByteStreams = ReadableWritablePair<Uint8Array, Uint8Array>;

/** Text worth compressing: pages, the data SvelteKit streams to them, and JSON. */
const COMPRESSIBLE = /^(text\/|application\/json)/;

/**
 * The response gzipped when the client takes gzip. adapter-node only serves the static files
 * compressed, so pages went out at full size. Every chunk is flushed as it's written, so a page
 * that streams still shows before its slow parts arrive.
 */
export function gzipped(request: Request, response: Response): Response {
	const accepted = /\bgzip\b/.test(request.headers.get('accept-encoding') ?? '');
	const compressible = COMPRESSIBLE.test(response.headers.get('content-type') ?? '');

	if (!response.body || !accepted || !compressible || response.headers.has('content-encoding')) return response;

	const headers = new Headers(response.headers);
	headers.set('content-encoding', 'gzip');
	headers.delete('content-length');
	headers.append('vary', 'accept-encoding');

	// SAFETY: Duplex.toWeb gives Node's web streams, the same classes as the DOM's at runtime (only
	// their typings differ), carrying the Buffers zlib writes, which are Uint8Arrays.
	const gzip = Duplex.toWeb(createGzip({ flush: constants.Z_SYNC_FLUSH })) as ByteStreams;

	return new Response(response.body.pipeThrough(gzip), {
		status: response.status,
		statusText: response.statusText,
		headers
	});
}
