import { Agent } from '@atproto/api';
import { fail, redirect } from '@sveltejs/kit';
import { grantsEmail, OptionalScopeUnsupported } from '#lib/server/atproto/oauth.ts';
import { requireViewer } from '#lib/server/http.ts';
import { KIND_TOPICS } from '#lib/server/notifications/kinds.ts';
import type { NotificationChannel, NotificationTopic } from '#lib/types.ts';
import type { Action, Actions, PageServerLoad } from './$types';

/** How asking the account's host for the address went, from the query the sign-in callback adds. */
const returned = (value: string | null): 'declined' | 'limited' | null =>
	value === 'declined' || value === 'limited' ? value : null;

const isTopic = (value: FormDataEntryValue): value is NotificationTopic => KIND_TOPICS.some((topic) => topic === value);

/** What each channel's result says, so it shows on the right card. */
const done = (channel: NotificationChannel, success: string) => ({ channel, success });

const failed = (status: number, channel: NotificationChannel, error: string) => fail(status, { channel, error });

const WORDS: Record<NotificationChannel, { picked: string; none: string; test: string; off: string }> = {
	email: {
		picked: 'Saved.',
		none: 'Saved. Nothing will be emailed until you pick something.',
		test: 'Sent. Check your email.',
		off: "Email's off. We've deleted your address."
	},
	dm: {
		picked: 'Saved.',
		none: "Saved. Nothing will be DM'd until you pick something.",
		test: 'Sent. Check your Bluesky chats.',
		off: 'DMs are off.'
	}
};

const topicsFor =
	(channel: NotificationChannel): Action =>
	async ({ locals, url, request }) => {
		const viewer = requireViewer(locals, url);
		const topics = (await request.formData()).getAll('topic').filter(isTopic);
		await locals.app.notificationChannels.setTopics(viewer.did, channel, topics);

		return done(channel, topics.length ? WORDS[channel].picked : WORDS[channel].none);
	};

const testFor =
	(channel: NotificationChannel): Action =>
	async ({ locals, url }) => {
		const viewer = requireViewer(locals, url);
		const { app } = locals;

		if (!app.limits.testSend.take(`test:${viewer.did}`)) {
			return failed(429, channel, "That's enough tests for now. Try again in an hour.");
		}

		const outcome = await app.outbox.sendTest(viewer.did, channel);

		if (outcome?.kind === 'sent') return done(channel, WORDS[channel].test);

		if (!outcome) return failed(400, channel, 'Turn it on first.');

		if (outcome.kind === 'rejected') return failed(400, channel, "It didn't go through, so it's paused.");

		return failed(502, channel, "We couldn't send it. Try again later.");
	};

const offFor =
	(channel: NotificationChannel): Action =>
	async ({ locals, url }) => {
		const viewer = requireViewer(locals, url);
		await locals.app.notificationChannels.disable(viewer.did, channel);

		return done(channel, WORDS[channel].off);
	};

export const load: PageServerLoad = async ({ locals, url }) => {
	const viewer = requireViewer(locals, url);
	const { app } = locals;
	const dm = await app.notificationChannels.view(viewer.did, 'dm');

	return {
		topics: KIND_TOPICS,
		email: {
			available: app.outbox.configured.includes('email'),
			channel: await app.notificationChannels.view(viewer.did, 'email'),
			returned: returned(url.searchParams.get('email'))
		},
		dm: {
			available: Boolean(app.dm),
			channel: dm,
			// Streamed: Bluesky chat can be slow, and the rest of the page doesn't wait on it.
			availability: app.dm ? app.dm.availability(viewer.did) : null,
			root: app.rootAccount.handle ?? app.rootDid
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
			return failed(429, 'email', 'Too many tries. Wait a few minutes, then try again.');
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
				return failed(429, 'email', "We can't send a confirmation link to that address right now. Try again tomorrow.");
			}

			const channel = read === 'read' ? await app.notificationChannels.view(viewer.did, 'email') : null;

			if (channel?.pausedReason === 'email_unverified') {
				return done('email', 'We emailed you a link. Open it to turn email on.');
			}

			if (channel) return done('email', channel.status === 'active' ? "Email's on." : 'Checked your address.');
		}

		let to: URL;
		const state = JSON.stringify({ next: '/dashboard/inbox/settings', intent: 'email' });

		try {
			to = await app.oauth.authorize(viewer.did, state, { account, optional: 'email' });
		} catch (error) {
			log.info({ err: error }, 'could not ask for the email address');

			if (error instanceof OptionalScopeUnsupported) {
				return failed(400, 'email', "Your account's server can't share your email address with apps.");
			}

			return failed(502, 'email', "We couldn't reach your account's server. Try again in a minute.");
		}

		redirect(303, to, { external: true });
	},

	emailTopics: topicsFor('email'),
	emailTest: testFor('email'),
	emailOff: offFor('email'),

	/** Turns DMs on, or holds them until the viewer's Bluesky chat settings let the root's DMs through. */
	dmOn: async ({ locals, url }) => {
		const viewer = requireViewer(locals, url);
		const { app } = locals;

		if (!app.dm) return failed(400, 'dm', "Bluesky DMs aren't set up on this site.");

		if (!app.limits.optIn.take(`optIn:${viewer.did}`)) {
			return failed(429, 'dm', 'Too many tries. Wait a few minutes, then try again.');
		}

		const availability = await app.dm.availability(viewer.did);

		if (availability === 'unavailable')
			return failed(502, 'dm', "We couldn't reach Bluesky chat. Try again in a minute.");
		await app.notificationChannels.enableDm(viewer.did, availability === 'ok');

		return availability === 'ok'
			? done('dm', 'DMs are on.')
			: failed(400, 'dm', "Bluesky chat won't deliver our DMs to you yet.");
	},
	dmTopics: topicsFor('dm'),
	dmTest: testFor('dm'),
	dmOff: offFor('dm')
};
