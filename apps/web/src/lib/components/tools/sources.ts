import type { Picture } from '@sveltejs/enhanced-img';
import type { Capture } from '#lib/tools/types.ts';

/**
 * The formats a capture's `<source>`s offer. `?enhanced` also makes a JPEG or PNG at every width,
 * but every browser the site supports takes one of these, so that format goes out only as the
 * `<img>`'s one fallback file.
 */
export const FORMATS = ['avif', 'webp'];

const slim = ({ sources, img }: Picture): Picture => ({
	sources: Object.fromEntries(FORMATS.flatMap((format) => (sources[format] ? [[format, sources[format]]] : []))),
	img
});

/** A capture with only the sources it's shown with, for a page's data. */
export const slimCapture = ({ picture, phone, alt }: Capture): Capture => ({
	picture: slim(picture),
	...(phone && { phone: slim(phone) }),
	alt
});
