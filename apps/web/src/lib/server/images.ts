import { error } from '@sveltejs/kit';
import projectDefs from '../../../../../lexicons/computer/localnetwork/defs.json' with { type: 'json' };
import epkDefs from '../../../../../lexicons/computer/localnetwork/epk/defs.json' with { type: 'json' };
import type { Logger } from './logger';

/** The types a press kit photo can be, as its lexicon's `accept` lists them. */
export const PHOTO_TYPES: ReadonlySet<string> = new Set(epkDefs.defs.photo.properties.image.accept);

/** The types a project's avatar can be, as its lexicon's `accept` lists them. */
export const AVATAR_TYPES: ReadonlySet<string> = new Set(projectDefs.defs.projectProfile.properties.avatar.accept);

/**
 * Serves a stored image from the site's origin, or 404s unless its type is one `accept` lists. The type is whatever
 * the blob's writer said, and a PDS doesn't hold it to the lexicon, so an SVG carrying a script would otherwise run as
 * one of our pages. The CSP's sandbox covers the image opened on its own; an `<img>` ignores it.
 */
export function imageResponse(
	body: BodyInit,
	mimeType: string,
	accept: ReadonlySet<string>,
	{ cacheControl, download = false, log }: { cacheControl: string; download?: boolean; log: Logger }
): Response {
	// MIME types match whatever their case.
	const type = mimeType.toLowerCase();

	if (!accept.has(type)) {
		log.warn({ mimeType }, 'refused to serve a stored image of a type its lexicon does not accept');
		error(404, 'Not found');
	}

	return new Response(body, {
		headers: {
			'content-type': type,
			'cache-control': cacheControl,
			'content-disposition': download ? 'attachment' : 'inline',
			'content-security-policy': "default-src 'none'; sandbox",
			'x-content-type-options': 'nosniff'
		}
	});
}
