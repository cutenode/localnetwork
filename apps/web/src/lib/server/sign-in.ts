import { isValidHandle } from '@atproto/syntax';
import hostList from '#lib/hosts.json' with { type: 'json' };
import type { AccountHost, ProjectView } from '#lib/types.ts';
import { isLoopback } from './atproto/oauth';
import type { Config } from './config';
import type { Logger } from './logger';

const HOSTS: AccountHost[] = hostList.hosts;

/**
 * The hosts "Make an account" offers (hosts.json). On a dev network its own PDS comes first, since an account made
 * anywhere else lives in the real PLC directory, which the dev network doesn't read.
 */
export function accountHosts(config: Pick<Config, 'dev' | 'root'>): AccountHost[] {
	const service = config.root.service;

	if (!config.dev || !service || !isLoopback(service)) return HOSTS;

	const local = { name: 'This dev network', domain: 'test', service, country: 'your computer' };

	return [local, ...HOSTS];
}

/**
 * Why someone was sent to sign in, from where they go after (`next`), finishing "Sign in … to <purpose>." Null when
 * the page they came from doesn't say. A project's page, or the inquiry form for it, is about booking it, unless
 * they came from its Follow button (`#follow`). A profile's Follow button names the handle in its address, which
 * says who without looking anyone up.
 */
export async function signInPurpose(
	next: string,
	deps: { bySlug: (slug: string) => Promise<ProjectView | null>; log: Logger }
): Promise<string | null> {
	const url = new URL(next, 'http://same.site');

	if (url.pathname === '/shows' && url.searchParams.get('view') === 'following') return 'see shows from DJs you follow';

	const handle = url.hash === '#follow' ? url.pathname.match(/^\/@([^/]+)$/)?.[1] : undefined;

	if (handle) return isValidHandle(handle) ? `follow @${handle}` : null;

	const slug =
		url.pathname.match(/^\/projects\/([^/]+)/)?.[1] ??
		(url.pathname === '/dashboard/bookings/new' ? url.searchParams.get('project') : null);

	if (!slug) return null;
	let project: ProjectView | null;

	try {
		project = await deps.bySlug(slug);
	} catch (error) {
		deps.log.debug('could not name the project behind a sign-in', { err: error, slug });

		return null;
	}

	// Only a public project's name: a hidden or archived one stays private to its managers and staff.
	if (project?.status !== 'active') return null;

	return `${url.hash === '#follow' ? 'follow' : 'book'} ${project.profile.name}`;
}
