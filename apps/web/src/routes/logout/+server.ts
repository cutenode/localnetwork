import { redirect } from '@sveltejs/kit';
import { SESSION_COOKIE } from '#lib/server/auth/session.ts';
import type { RequestHandler } from './$types';

export const POST: RequestHandler = async ({ locals, cookies }) => {
	const { app, viewer, sessionToken } = locals;

	if (viewer) {
		// Write the audit entry (both copies) before the OAuth session is revoked.
		const recorded = app.audit.record(
			{
				actor: viewer.did,
				actorRole: viewer.isAdmin ? 'admin' : viewer.isStaff ? 'moderator' : 'member',
				action: 'auth.logout',
				subject: viewer.did,
				subjectKind: 'account',
				summary: 'Signed out'
			},
			{ awaitDelivery: true }
		);

		await Promise.race([recorded, new Promise((resolve) => setTimeout(resolve, 5_000))]);
		await app.oauth.revoke(viewer.did);
	}

	await app.sessions.destroy(sessionToken);
	cookies.delete(SESSION_COOKIE, { path: '/' });
	redirect(303, '/');
};
