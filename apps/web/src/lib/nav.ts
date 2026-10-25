import { accountLabel, pluralize } from '#lib/format.ts';
import type { Viewer } from '#lib/types.ts';

/** A header link and the key that opens it. `also` lists other paths it stays current on; `badge` a count waiting there. */
export type NavLink = { key: string; href: string; label: string; also?: string[]; badge?: number };

/**
 * What the site header shows a viewer: the four destinations, then For DJs and Sign in, or once
 * signed in, Dashboard and an account menu (the staff console in it for staff).
 */
export function siteNav(viewer: Viewer | null, pendingRequests: number) {
	const links: NavLink[] = [
		{ key: 'F', href: '/projects', label: 'Find DJs', also: ['/match'] },
		{ key: 'W', href: '/shows', label: 'Shows' },
		{ key: 'R', href: '/charts', label: 'Charts', also: ['/artists'] },
		{ key: 'O', href: '/tools', label: 'Tools' }
	];

	if (!viewer) {
		const actions: NavLink[] = [
			{ key: 'D', href: '/for-djs', label: 'For DJs' },
			{ key: 'S', href: '/login', label: 'Sign in' }
		];

		return { links, actions, account: undefined };
	}

	const handle = accountLabel(viewer);

	const staff: NavLink[] = viewer.isStaff
		? [{ key: 'T', href: '/admin', label: 'Staff console', badge: pendingRequests }]
		: [];

	return {
		links,
		actions: [{ key: 'U', href: '/dashboard', label: 'Dashboard' }],
		account: {
			handle,
			name: handle.replace(/^@/, '').split('.')[0],
			links: staff,
			alert: viewer.isStaff && pendingRequests ? `${pluralize(pendingRequests, 'request')} waiting` : undefined,
			signOut: '/logout'
		}
	};
}
