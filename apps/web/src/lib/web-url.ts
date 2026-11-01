/** Whether `value` is a full http(s) address, the only kind of link pages make clickable. */
export function isWebUrl(value: string): boolean {
	return URL.canParse(value) && ['http:', 'https:'].includes(new URL(value).protocol);
}

/**
 * `url` when it's a full http(s) address, else undefined, for an `href`. People write their profiles, sets and press
 * kits into their own repos, where nothing stops a `javascript:` or `data:` link, so every address someone wrote goes
 * through here before a page makes it clickable.
 */
export function webHref(url: string | null | undefined): string | undefined {
	return url && isWebUrl(url) ? url : undefined;
}
