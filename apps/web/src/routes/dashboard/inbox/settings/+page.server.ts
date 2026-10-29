import { Agent } from '@atproto/api';
import { fail, redirect } from '@sveltejs/kit';
import { grantsEmail, OptionalScopeUnsupported } from '#lib/server/atproto/oauth.ts';
import { requireViewer } from '#lib/server/http.ts';
import { KIND_TOPICS } from '#lib/server/notifications/kinds.ts';
import type { NotificationTopic } from '#lib/types.ts';
import type { Actions, PageServerLoad } from './$types';

/** How asking the account's host for the address went, from the query the sign-in callback adds. */
const returned = (value: string | null): 'declined' | 'limited' | null =>
	value === 'declined' || value === 'limited' ? value : null;

const isTopic = (value: FormDataEntryValue): value is NotificationTopic => KIND_TOPICS.some((topic) => topic === value);

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const { app } = locals;

	return {
		topics: KIND_TOPICS,
		email: {
			available: app.outbox.configured.includes('email'),
			channel: await app.notificationChannels.view(viewer.did, 'email'),
			returned: returned(url.searchParams.get('email'))
		}
	};
};

export const actions: Actions = {
	/**
	 * Reads the viewer's address with the session they have when it already allows that, and otherwise sends them to
	 * their account's host to allow it, coming back through the sign-in callback.
	 */
	emailOn: async ({ locals, url }) => {
		const viewer = requireViewer(locals, url);
		const { app, log } = locals;

		if (!app.limits.optIn.take(`optIn:${viewer.did}`)) {
			return fail(429, { error: 'Too many tries. Wait a few minutes, then try again.' });
		}

		const account = await app.accounts.get(viewer.did);
		const session = grantsEmail(account?.grantedScope, viewer.did) ? await app.oauth.restore(viewer.did) : null;

		if (account?.grantedScope && session) {
			const read = await app.notificationChannels.onSignIn({
				did: viewer.did,
				scope: account.grantedScope,
				optIn: true,
				session: async () => (await new Agent(session).com.atproto.server.getSession()).data
			});

			if (read === 'limited') {
				return fail(429, { error: "We can't send a confirmation link to that address right now. Try again tomorrow." });
			}

			const channel = read === 'read' ? await app.notificationChannels.view(viewer.did, 'email') : null;

			if (channel?.pausedReason === 'email_unverified')
				return { success: 'We emailed you a link. Open it to turn email on.' };

			if (channel) return { success: channel.status === 'active' ? "Email's on." : 'Checked your address.' };
		}

		let to: URL;
		const state = JSON.stringify({ next: '/dashboard/inbox/settings', intent: 'email' });

		try {
			to = await app.oauth.authorize(viewer.did, state, { account, optional: 'email' });
		} catch (error) {
			log.info({ err: error }, 'could not ask for the email address');

			if (error instanceof OptionalScopeUnsupported) {
				return fail(400, { error: "Your account's server can't share your email address with apps." });
			}

			return fail(502, { error: "We couldn't reach your account's server. Try again in a minute." });
		}

		redirect(303, to, { external: true });
	},

	emailTopics: async ({ locals, url, request }) => {
		const viewer = requireViewer(locals, url);
		const topics = (await request.formData()).getAll('topic').filter(isTopic);
		await locals.app.notificationChannels.setTopics(viewer.did, 'email', topics);

		return { success: topics.length ? 'Saved.' : 'Saved. Nothing will be emailed until you pick something.' };
	},

	emailTest: async ({ locals, url }) => {
		const viewer = requireViewer(locals, url);
		const { app } = locals;

		if (!app.limits.testSend.take(`test:${viewer.did}`)) {
			return fail(429, { error: "That's enough tests for now. Try again in an hour." });
		}

		const outcome = await app.outbox.sendTest(viewer.did, 'email');

		if (outcome?.kind === 'sent') return { success: 'Sent. Check your email.' };

		if (!outcome) return fail(400, { error: 'Turn email on first.' });

		if (outcome.kind === 'rejected') return fail(400, { error: "Your address didn't take it, so email is paused." });

		return fail(502, { error: "We couldn't send it. Try again later." });
	},

	emailOff: async ({ locals, url }) => {
		const viewer = requireViewer(locals, url);
		await locals.app.notificationChannels.disable(viewer.did, 'email');

		return { success: "Email's off. We've deleted your address." };
	}
};
