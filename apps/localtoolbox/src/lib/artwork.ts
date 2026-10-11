// Full-size artwork URLs, derived from the thumbnails a page already showed.
//
// SoundCloud serves every artwork at multiple sizes, addressed by the last
// "-<size>" token of the filename:
//
//   https://i1.sndcdn.com/artworks-<hash>-t120x120.jpg    playlist row thumb
//   https://i1.sndcdn.com/artworks-<hash>-t500x500.jpg    og:image size
//   https://i1.sndcdn.com/artworks-<hash>-t1080x1080.jpg  player hero banner
//
// SoundCloud's own player addresses renditions purely by rewriting this
// token, so no request is needed to "discover" the full-size URL. Legacy
// named sizes (large, crop, …) are matched too in case an old page serves
// one, but modern pages use the tNxN form.

import type { Track } from '$lib/types';

const SIZE_TOKEN = /-(t\d+x\d+|original|large|crop|badge|small|tiny|mini)(\.(?:jpg|jpeg|png|gif))$/i;

/** The largest rendition SoundCloud serves, and the one a click asks for. */
export const FULL_SIZE = 't1080x1080';

/** @param size a size token, as FULL_SIZE is */
export function artworkUrlAtSize(url: string, size: string) {
	// Unrecognized URL shape: pass it through untouched rather than mangle it.
	if (!SIZE_TOKEN.test(url)) return url;

	return url.replace(SIZE_TOKEN, `-${size}$2`);
}

/** The full-size address of a thumbnail the page rendered. */
export const fullSizeArtwork = (url: string) => artworkUrlAtSize(url, FULL_SIZE);

/**
 * The cover to show for a track: the thumbnail its row rendered, else the one
 * SoundCloud named when it was asked about the track.
 *
 * A page lazy-loads its thumbnails as they scroll into view, so a list read
 * without scrolling all the way through hands over rows with no artwork URL —
 * and those rows drew a placeholder for tracks whose artwork plainly exists.
 * The details every such track is looked up for carry the address of it, at no
 * extra request, so that stands in. The row's own URL wins where there is one:
 * it is the rendition the browser already has.
 *
 * Entries put on the list before the details carried a cover have none stored
 * on them; this is also what fills those in, as soon as anything asks about
 * the track again.
 *
 * @returns '' where neither has one
 */
export const artworkOf = (track: Track | null | undefined) => track?.artworkUrl || track?.details?.artworkUrl || '';
