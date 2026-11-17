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
 * An act's, party's or venue's page: `/<kind>/<key>/<name>`. Names repeat, so the key is what finds it and the name
 * is only there to read (ADR 0016). A name with nothing to keep leaves just the key.
 */
export function spacePagePath(kind: SpacePageKind, key: string, name: string): string {
	const slug = nameSlug(name);

	return slug ? `/${kind}/${key}/${slug}` : `/${kind}/${key}`;
}
