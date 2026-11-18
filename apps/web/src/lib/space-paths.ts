/** The kinds of space with a public page, which its key addresses. */
type SpacePageKind = 'act' | 'party' | 'venue';

/** `input` as an address part: lowercase letters, numbers and dashes, at most 48. Empty when nothing's left. */
export function nameSlug(input: string): string {
	return input
		.normalize('NFKD')
		.replace(/[̀-ͯ]/g, '')
		.toLowerCase()
		.replace(/&/g, ' and ')
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '')
		.slice(0, 48)
		.replace(/-+$/g, '');
}

/**
 * `path` with `name` after it, only there to read. A name with nothing to keep leaves `path` alone, and so does one
 * that reads `new`, since `/party/<key>/new` is where a party's next event is made.
 */
function named(path: string, name: string): string {
	const slug = nameSlug(name);

	return slug && slug !== 'new' ? `${path}/${slug}` : path;
}

/**
 * An act's, party's or venue's page: `/<kind>/<key>/<name>`. Names repeat, so the key is what finds it and the name
 * is only there to read (ADR 0016).
 */
export function spacePagePath(kind: SpacePageKind, key: string, name: string): string {
	return named(`/${kind}/${key}`, name);
}

/** An event's page, under its party's: `/party/<key>/<event>/<name>`, where the event's record key finds it. */
export function eventPagePath(partyKey: string, rkey: string, name: string): string {
	return named(`/party/${partyKey}/${rkey}`, name);
}
