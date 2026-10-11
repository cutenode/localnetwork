// Handing a file to the browser, and working out what to call it.
//
// A transfer started here is the browser's from then on: it outlives the popup,
// which is the point — a page of gates can be set going and the window closed.

// What Chrome will not take in a filename, and what a name has to survive to
// be handed over as one.
//
// chrome.downloads.download() refuses the whole call — "Invalid filename" —
// rather than cleaning a name up, and it holds every name to what Windows
// would accept, whatever the machine actually is. So the characters below go
// before the name is offered, or the file is lost over its title:
//
//   the separators, and the shell metacharacters `"*:<>?|` that Windows
//   reserves — the ones an artist's own filename arrives full of, because
//   "Track *FREE DL*" and "Remix?" are how these files get named;
const ILLEGAL = /[/\\"*:<>?|]/g;

//   the control characters, the invisible format characters — soft hyphens,
//   zero-width spaces, bidi marks — that ride along unseen in a title pasted
//   out of a page, and any half of a character left over from a cut;
const INVISIBLE = /[\p{Cc}\p{Cf}\p{Cs}]/gu;

//   dots and spaces at either end, which Windows strips and Chrome therefore
//   rejects rather than silently renaming;
const ENDS = { leading: /^[.\s]+/, trailing: /(?:[.\s]|\p{Cs})+$/u };

//   and the device names, which are reserved whatever follows the dot.
const RESERVED = /^(?:con|prn|aux|nul|com\d|lpt\d)(?:\.|$)/i;

/** A single path segment Chrome will accept: no separators, no leading dots. */
export function safeName(name: string) {
	const cleaned = name.replace(INVISIBLE, '').replace(ILLEGAL, '-').replace(ENDS.leading, '');
	// A name that is only a device name would be one; a prefix is enough to
	// stop it being, and goes on before the cut so the cut stays the last word.
	const named = RESERVED.test(cleaned) ? `_${cleaned}` : cleaned;

	// The cut can land mid-character or leave a trailing dot, so the ends are
	// tidied after it rather than before.
	return named.slice(0, 180).replace(ENDS.trailing, '');
}

// What the file is actually called, in the order the answer is most likely to
// be right: the Content-Disposition a signed link carries (Hypeddit and
// Droploud both sign one in), then the last path segment, if it has an
// extension worth keeping.
export function filenameOf(fileUrl: string) {
	let parsed;

	try {
		parsed = new URL(fileUrl);
	} catch (_e) {
		return '';
	}

	const disposition = parsed.searchParams.get('response-content-disposition');
	const signed = disposition && /filename\*?=(?:UTF-8'')?"?([^";]+)"?/i.exec(disposition);

	if (signed) {
		try {
			return safeName(decodeURIComponent(signed[1]));
		} catch (_e) {
			return safeName(signed[1]);
		}
	}

	const last = parsed.pathname.slice(parsed.pathname.lastIndexOf('/') + 1);

	if (!AUDIO.test(last)) return '';

	try {
		return safeName(decodeURIComponent(last));
	} catch (_e) {
		return safeName(last);
	}
}

// The extensions a free download comes as. Used both for naming a file and —
// in looksLikeFile() below — for deciding whether a link is one at all.
const AUDIO = /\.(mp3|wav|aiff?|flac|m4a|ogg|opus|alac|wma|zip|rar|7z)$/i;

/**
 * Whether a URL is a file to save rather than a page to open.
 *
 * Several of these sites end in "here is where the download lives", and where
 * it lives is sometimes a file and sometimes a shop, a Patreon post or a
 * Dropbox folder. Only the first can be handed to the downloader; the rest are
 * pages, and the panel offers to open them instead.
 */
export function looksLikeFile(url: string) {
	let parsed;

	try {
		parsed = new URL(url);
	} catch (_e) {
		return false;
	}

	if (AUDIO.test(parsed.pathname)) return true;
	// Dropbox and Drive serve a page unless asked for the bytes.
	const host = parsed.hostname.replace(/^www\./, '');

	if (host.endsWith('dropbox.com')) return parsed.searchParams.get('dl') === '1';

	if (host === 'drive.google.com') return parsed.searchParams.get('export') === 'download';

	return false;
}

// Chrome's own words for a name it will not take. It is a constant in the
// extensions API rather than anything translated, so matching it is safe —
// and it is matched rather than assumed, so a disk that is full, a blocked
// URL or a cancelled download still fails as itself.
const NAME_REFUSED = /invalid\s*file\s*name/i;

/**
 * Hands a file to the browser's downloader.
 *
 * A name we already know goes in the site's own folder under the download
 * directory; without one, Chrome takes the name from the response and its own
 * default folder, because a folder can only be given alongside a filename.
 *
 * If Chrome refuses the name anyway — safeName() covers what it documents, not
 * every rule it has — the file is asked for again without one. The bytes are
 * the point; the name is a courtesy, and it lands in the default folder under
 * whatever the response calls it rather than not landing at all.
 *
 * @returns the download's id and the name it was given, '' where Chrome chose one; `renamed`
 *   when Chrome refused ours
 */
export async function save(
	fileUrl: string,
	{ subfolder = '', filename = '' }: { subfolder?: string; filename?: string } = {}
): Promise<{ id: number; name: string; renamed?: true }> {
	// A name the site gave, if it survives being made safe; the link's own if it
	// does not, since a title that was all punctuation is now nothing at all.
	const name = safeName(filename) || filenameOf(fileUrl);
	const options: chrome.downloads.DownloadOptions = { url: fileUrl };

	if (name) options.filename = subfolder ? `${subfolder}/${name}` : name;

	try {
		const id = await chrome.downloads.download(options);

		return { id, name };
	} catch (error) {
		if (!options.filename || !(error instanceof Error && NAME_REFUSED.test(error.message))) throw error;
		const id = await chrome.downloads.download({ url: fileUrl });

		return { id, name: '', renamed: true };
	}
}
