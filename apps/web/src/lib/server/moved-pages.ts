/** Pages that moved. Emails, DMs and old links still reach the old paths, so these stay as long as those might. */
const MOVED = [
	['/dashboard/activity', '/dashboard/account/activity'],
	['/dashboard/data', '/dashboard/account/data'],
	['/dashboard/requests', '/dashboard/account/requests'],
	['/dashboard/inbox/settings', '/dashboard/account/notifications']
] as const;

/** Pages that moved while what's under them stayed: Find DJs left `/projects`, where project pages still are. */
const MOVED_ALONE = [['/projects', '/djs']] as const;

/** Where `url` lives now, keeping what's under it and its query, or null when it hasn't moved. */
export function movedTo({ pathname, search }: URL): string | null {
	const alone = MOVED_ALONE.find(([from]) => pathname === from);

	if (alone) return `${alone[1]}${search}`;

	const moved = MOVED.find(([from]) => pathname === from || pathname.startsWith(`${from}/`));

	return moved ? `${moved[1]}${pathname.slice(moved[0].length)}${search}` : null;
}
