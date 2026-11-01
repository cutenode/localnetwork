import { redirect } from '@sveltejs/kit';
import { Agent } from '@atproto/api';
import { z } from 'zod';
import { grantsPublicRecords, grantsSpaces } from '#lib/server/atproto/oauth.ts';
import { AgentRepoClient } from '#lib/server/atproto/repo-client.ts';
import { SESSION_COOKIE } from '#lib/server/auth/session.ts';
import { safeNext, withQuery } from '#lib/server/http.ts';
import type { RequestHandler } from './$types';

/** The app state the login action passes through `authorize`, and what the sign-in was for when it asked for more. */
const loginState = z.object({ next: z.string().optional(), intent: z.enum(['email']).optional() });

/** The app state from `authorize`, or undefined when there's none or it doesn't parse. */
function parseState(state: string | null | undefined) {
	try {
		return loginState.safeParse(JSON.parse(state ?? '{}')).data;
	} catch {
		return undefined;
	}
}

export const GET: RequestHandler = async ({ url, locals, cookies, request }) => {
	const { app, log } = locals;

	const failure = url.searchParams.get('error');

	if (failure) {
		const started = parseState(await app.oauth.failedState(url.searchParams));

		// Turning email on, they declined sharing the address and are still signed in, so they go back to say so.
		if (failure === 'access_denied' && started?.intent === 'email') {
			redirect(303, withQuery('/dashboard/inbox/settings', { email: 'declined' }));
		}

		redirect(303, `/login?error=${failure === 'access_denied' ? 'denied' : 'login'}`);
	}

	let did: string;
	let next = '/dashboard';

	try {
		const { session, state, scope } = await app.oauth.callback(url.searchParams);
		did = session.did;
		const parsed = parseState(state);
		const intent = parsed?.intent;
		next = safeNext(parsed?.next);

		// Only a grant of every space write the app asks for lets it write there as the user. Anything less (no
		// spaces support, or scopes the user declined) falls back to the root account writing on their behalf.
		const spacesCapable = grantsSpaces(scope, did, app.rootDid);
		const identity = await app.identity.resolve(did).catch(() => null);
		await app.accounts.recordLogin({
			did,
			handle: identity?.handle ?? null,
			pdsUrl: identity?.pds ?? null,
			grantedScope: scope,
			spacesCapable,
			publicWrites: grantsPublicRecords(scope, did, app.rootDid)
		});

		const email = await app.notificationChannels.onSignIn({
			did,
			scope,
			optIn: intent === 'email',
			session: async () => (await new Agent(session).com.atproto.server.getSession()).data
		});

		if (email === 'declined' || email === 'limited') next = withQuery(next, { email });
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
	} catch (error) {
		log.warn({ err: error }, 'OAuth callback failed');
		redirect(303, '/login?error=login');
	}

	redirect(303, next);
};
