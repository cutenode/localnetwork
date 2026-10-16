import { redirect } from '@sveltejs/kit';
import { Agent } from '@atproto/api';
import { z } from 'zod';
import { grantsAll } from '#lib/server/atproto/oauth.ts';
import { AgentRepoClient } from '#lib/server/atproto/repo-client.ts';
import { SESSION_COOKIE } from '#lib/server/auth/session.ts';
import { safeNext } from '#lib/server/http.ts';
import type { RequestHandler } from './$types';

/** The app state the login action passes through `authorize`. */
const loginState = z.object({ next: z.string().optional() });

export const GET: RequestHandler = async ({ url, locals, cookies, request }) => {
	const { app, log } = locals;

	if (url.searchParams.get('error')) {
		redirect(303, `/login?error=${url.searchParams.get('error') === 'access_denied' ? 'denied' : 'login'}`);
	}

	let did: string;
	let next = '/me';

	try {
		const { session, state, scope } = await app.oauth.callback(url.searchParams);
		did = session.did;

		try {
			next = safeNext(loginState.safeParse(JSON.parse(state ?? '{}')).data?.next);
		} catch {
			next = '/me';
		}

		// Only a grant of everything the app asks for lets it write as the user. Anything less (no spaces
		// support, or scopes the user declined) falls back to the root account writing on their behalf.
		const spacesCapable = grantsAll(scope, app.oauth.scopes.full, did);
		const identity = await app.identity.resolve(did).catch(() => null);
		await app.accounts.recordLogin({
			did,
			handle: identity?.handle ?? null,
			pdsUrl: identity?.pds ?? null,
			grantedScope: scope,
			spacesCapable
		});
		const previous = cookies.get(SESSION_COOKIE);

		if (previous) await app.sessions.destroy(previous);
		const token = await app.sessions.create(did, request.headers.get('user-agent'));
		cookies.set(SESSION_COOKIE, token, {
			path: '/',
			httpOnly: true,
			sameSite: 'lax',
			secure: app.config.publicUrl.startsWith('https:'),
			maxAge: app.sessions.maxAgeSeconds
		});
		const roles = await app.staff.roles(did);
		await app.audit.record(
			{
				actor: did,
				actorRole: roles.includes('admin') ? 'admin' : roles.length ? 'moderator' : 'member',
				action: 'auth.login',
				subject: did,
				subjectKind: 'account',
				summary: `Signed in${identity?.handle ? ` as @${identity.handle}` : ''}`,
				details: { spacesCapable, pds: identity?.pds }
			},
			{ actorClient: spacesCapable ? new AgentRepoClient(did, new Agent(session)) : null }
		);
		// Requests their press kit rules approved while they were away get their grants now.
		const viewer = await app.viewer(did);

		if (viewer)
			void app.epks
				.settle(viewer)
				.catch((cause) => log.warn({ err: cause }, 'could not write waiting press kit grants'));
	} catch (error) {
		log.warn({ err: error }, 'OAuth callback failed');
		redirect(303, '/login?error=login');
	}

	redirect(303, next);
};
