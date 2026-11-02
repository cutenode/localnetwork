import sharp from 'sharp';
import { kindLabel } from '#lib/format.ts';
import type { PerformerCardProps } from '#lib/og/cards/PerformerCard.svelte';
import { ink } from '#lib/og/style.ts';
import type { ProjectView } from '#lib/types.ts';
import type { Core } from '../core';
import { AVATAR_MAX_BYTES, AVATAR_TYPES } from '../images';
import type { Logger } from '../logger';

/**
 * The most pixels a stored picture may have to be drawn on a card: more than any phone or most cameras take (a 7,700 ×
 * 5,200 photo), and far short of a decompression bomb, a small file that decodes to gigabytes.
 */
const MAX_INPUT_PIXELS = 40_000_000;

/**
 * A picture cropped to `width`×`height` as a JPEG data URL: a type Satori draws (it can't WebP), and only as many
 * pixels as the card shows. Null unless the bytes, whatever type they were stored as, are a picture of a type `accept`
 * lists (sharp would also read an SVG or a TIFF) within MAX_INPUT_PIXELS.
 */
export async function cardImage(
	bytes: Uint8Array,
	{ width, height, accept }: { width: number; height: number; accept: ReadonlySet<string> },
	log: Logger
): Promise<string | null> {
	try {
		const picture = sharp(bytes, { limitInputPixels: MAX_INPUT_PIXELS });
		const { format } = await picture.metadata();

		if (!accept.has(`image/${format}`)) {
			log.warn({ format }, "a card's picture isn't a type its lexicon accepts");

			return null;
		}

		const jpeg = await picture
			.rotate()
			.resize(width, height, { fit: 'cover' })
			.flatten({ background: ink.canvas })
			.jpeg({ quality: 85 })
			.toBuffer();

		return `data:image/jpeg;base64,${jpeg.toString('base64')}`;
	} catch (cause) {
		log.warn({ err: cause }, "couldn't read a picture for a card");

		return null;
	}
}

/** The bytes of a `data:` URL's base64 payload, as mix reports embed their covers. */
export const dataUrlBytes = (url: string): Uint8Array => Buffer.from(url.slice(url.indexOf(',') + 1), 'base64');

/**
 * A project's avatar, `size` pixels square, or null without one. A fetch that fails throws, so the request fails and
 * is tried again rather than a card without it being cached for good.
 */
export async function projectAvatar(
	core: Core,
	project: ProjectView,
	size: number,
	log: Logger
): Promise<string | null> {
	const cid = project.profile.avatar?.ref.$link;

	if (!cid || !core.fetchBlob) return null;
	const blob = await core.fetchBlob(core.rootDid, cid, AVATAR_MAX_BYTES);

	return cardImage(blob.bytes, { width: size, height: size, accept: AVATAR_TYPES }, log);
}

/** What a performer's card shows of `project`, with `avatar` as its picture. */
export const performerCard = ({ profile }: ProjectView, avatar: string | null): PerformerCardProps => ({
	name: profile.name,
	kind: kindLabel(profile.kind),
	city: profile.location.city,
	genres: profile.genres.slice(0, 4),
	avatar
});
