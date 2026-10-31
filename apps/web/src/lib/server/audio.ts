import { type Accept, BlobPermission } from '@atproto/oauth-scopes';
import recordingLexicon from '../../../../../lexicons/computer/localnetwork/dj/recording.json' with { type: 'json' };
import type { Logger } from './logger';

const { accept, maxSize } = recordingLexicon.defs.main.record.properties.audio;

/**
 * The types a recording's audio can be, as its lexicon's `accept` lists them (`audio/*` and `video/mp4`), matched the
 * way a PDS matches a `blob:` scope.
 */
// SAFETY: the lexicon's accept list isn't empty, and each entry is a MIME type or a `type/*` pattern.
export const RECORDING_TYPES = new BlobPermission(accept as [Accept, ...Accept[]]);

/** The largest recording served: the recording lexicon's `audio` maxSize, 300 MiB. */
export const RECORDING_MAX_BYTES = maxSize;

/** A plain answer for a refusal, so a browser shows the status rather than a media error page. */
export const refuse = (status: number, message: string, headers: Record<string, string> = {}) =>
	new Response(message, { status, headers: { 'content-type': 'text/plain; charset=utf-8', ...headers } });

/**
 * Serves a recording's audio from the site's origin, or 404s unless its type is one `accept` lists. The type is
 * whatever the blob's writer said, and a PDS doesn't hold it to the lexicon, so an HTML one would otherwise run as one
 * of our pages. A refused body is cancelled, so its upstream stops.
 */
export function audioResponse(
	body: ReadableStream<Uint8Array>,
	mimeType: string,
	{ status, headers, log }: { status: number; headers: Record<string, string>; log: Logger }
): Response {
	// MIME types match whatever their case.
	const type = mimeType.toLowerCase();

	if (!RECORDING_TYPES.matches({ mime: type })) {
		log.warn({ mimeType, status: 404 }, 'refused to serve a recording of a type its lexicon does not accept');
		void body.cancel();

		return refuse(404, 'Not found');
	}

	return new Response(body, {
		status,
		headers: {
			...headers,
			// A PDS names a blob's type by sniffing its bytes, and an M4A whose header doesn't brand it as audio comes back
			// as video/mp4, which the recording lexicon accepts for that reason. It's a recording all the same, so the
			// <audio> element (and anyone opening the link, who'd otherwise get a video page) gets audio/mp4.
			'content-type': type === 'video/mp4' ? 'audio/mp4' : type,
			// Opened on its own, it's a bare media document that loads nothing else. No `sandbox`: its opaque origin
			// would make the document's own request for the audio cross-origin, and Chromium refuses it.
			'content-security-policy': "default-src 'none'; media-src 'self'; style-src 'unsafe-inline'",
			'x-content-type-options': 'nosniff'
		}
	});
}
