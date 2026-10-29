import { audioResponse, refuse } from '../audio';
import type { Logger } from '../logger';
import type { RecordingAudio } from '../services/recordings';

/** A byte range of a file, both ends included. */
export type ByteRange = { start: number; end: number };

/**
 * The range a `Range` header asks for in a file of `size` bytes: null to send the whole file (no header, another unit,
 * a malformed one, or several ranges, which browsers playing audio don't ask for and RFC 9110 lets a server ignore),
 * or 'unsatisfiable' when it starts past the end.
 */
export function parseRange(header: string | null, size: number): ByteRange | 'unsatisfiable' | null {
	const match = header && /^bytes=\s*(\d*)-(\d*)\s*$/.exec(header);

	if (!match || (!match[1] && !match[2])) return null;

	if (!match[1]) {
		const suffix = Number(match[2]);

		return suffix === 0 ? 'unsatisfiable' : { start: Math.max(0, size - suffix), end: size - 1 };
	}

	const start = Number(match[1]);
	const end = match[2] ? Number(match[2]) : Infinity;

	if (end < start) return null;

	return start >= size ? 'unsatisfiable' : { start, end: Math.min(end, size - 1) };
}

/**
 * `length` bytes of `body` from `start`, read in order since the PDS can't seek: what comes before is read and
 * dropped, and the upstream is cancelled once the range is out, so a seek near the start doesn't download the rest.
 */
function slice(body: ReadableStream<Uint8Array>, start: number, length: number): ReadableStream<Uint8Array> {
	const reader = body.getReader();
	let skip = start;
	let left = length;

	return new ReadableStream({
		async pull(controller) {
			for (;;) {
				if (left === 0) {
					controller.close();
					await reader.cancel();

					return;
				}

				const { done, value } = await reader.read();

				if (done) {
					controller.error(new Error('the PDS sent less than the recording declares'));

					return;
				}

				if (skip >= value.length) {
					skip -= value.length;
					continue;
				}

				const chunk = value.subarray(skip, skip + left);
				skip = 0;
				left -= chunk.length;
				controller.enqueue(chunk);

				return;
			}
		},
		cancel: (reason) => reader.cancel(reason)
	});
}

export type AudioRequest = {
	did: string;
	cid: string;
	range: string | null;
	/** The declared audio of the recording that references the blob, or null when the site doesn't serve it. */
	audio: (did: string, cid: string) => Promise<RecordingAudio | null>;
	/** The account's PDS. */
	pds: (did: string) => Promise<string>;
	fetch: typeof globalThis.fetch;
	log: Logger;
};

/**
 * Streams a DJ's recording from their PDS through the site, for the set player: only a blob a recording record they
 * published references, served as the type (see audioResponse) and size that record declares, with Range answered here
 * since `com.atproto.sync.getBlob` ignores it. Nothing is stored; every request reads the PDS again, and browsers cache
 * the answers, since a CID's bytes never change.
 */
export async function recordingAudio(request: AudioRequest): Promise<Response> {
	const { did, cid } = request;
	const log = request.log.child({ did, cid, range: request.range });
	const audio = await request.audio(did, cid);

	if (!audio) {
		log.info({ status: 404 }, 'no recording references this audio');

		return refuse(404, 'Not found');
	}

	const range = parseRange(request.range, audio.size);

	if (range === 'unsatisfiable') {
		log.info({ status: 416 }, 'recording range past the end');

		return refuse(416, 'Range not satisfiable', { 'content-range': `bytes */${audio.size}` });
	}

	let upstream: Response;

	try {
		// Blobs come from the account's PDS until HappyView serves them (2.16 can't serve blob bytes).
		// ast-grep-ignore: no-direct-pds-read
		const url = new URL('/xrpc/com.atproto.sync.getBlob', await request.pds(did));
		url.searchParams.set('did', did);
		url.searchParams.set('cid', cid);
		upstream = await request.fetch(url, { signal: AbortSignal.timeout(15_000) });
	} catch (error) {
		log.warn({ err: error, status: 502 }, 'could not reach the PDS for a recording');

		return refuse(502, 'Recording unavailable');
	}

	if (!upstream.ok || !upstream.body) {
		const status = upstream.status === 404 ? 404 : 502;
		await upstream.body?.cancel();
		log.warn({ upstream: upstream.status, status }, 'the PDS did not serve a recording');

		return refuse(status, 'Recording unavailable');
	}

	const { start, end } = range ?? { start: 0, end: audio.size - 1 };
	const length = end - start + 1;
	const status = range ? 206 : 200;
	log.info({ status, bytes: length }, 'serving a recording');

	return audioResponse(slice(upstream.body, start, length), audio.mimeType, {
		status,
		headers: {
			'content-length': String(length),
			'accept-ranges': 'bytes',
			...(range && { 'content-range': `bytes ${start}-${end}/${audio.size}` }),
			etag: `"${cid}"`,
			'cache-control': 'public, max-age=31536000, immutable'
		},
		log
	});
}
