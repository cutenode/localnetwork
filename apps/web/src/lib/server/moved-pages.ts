/** Pages that moved. Emails, DMs and old links still reach the old paths, so these stay as long as those might. */
const MOVED = [
	['/dashboard/activity', '/dashboard/account/activity'],
	['/dashboard/data', '/dashboard/account/data'],
	['/dashboard/requests', '/dashboard/account/requests'],
	['/dashboard/inbox/settings', '/dashboard/account/notifications']
] as const;

/** Where `url` lives now, keeping what's under it and its query, or null when it hasn't moved. */
export function movedTo({ pathname, search }: URL): string | null {
	const moved = MOVED.find(([from]) => pathname === from || pathname.startsWith(`${from}/`));

	return moved ? `${moved[1]}${pathname.slice(moved[0].length)}${search}` : null;
}
