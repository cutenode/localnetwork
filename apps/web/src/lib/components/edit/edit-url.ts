/**
 * The page at `url` in edit mode (`?edit`) or out of it, its other query kept. A form action's own `?/name` goes too,
 * since a save lands back on the page it was sent from.
 */
export function editUrl(url: { pathname: string; search: string }, edit: boolean): string {
	const kept = [...new URLSearchParams(url.search)].filter(([key]) => key !== 'edit' && !key.startsWith('/'));
	const query = [new URLSearchParams(kept).toString(), edit ? 'edit' : ''].filter(Boolean).join('&');

	return `${url.pathname}${query ? `?${query}` : ''}`;
}
