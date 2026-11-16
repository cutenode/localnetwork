import { afterEach, beforeEach, expect, it } from 'vitest';
import { OptionalScopeUnsupported } from '#lib/server/atproto/oauth.ts';
import { silentLogger } from '#lib/server/logger.ts';
import { type ConfirmLimits, NotificationChannels, type SessionEmail } from '#lib/server/notifications/channels.ts';
import type { DmSender } from '#lib/server/notifications/dm.ts';
import { type ChannelSender, Outbox, type SendOutcome } from '#lib/server/notifications/outbox.ts';
import { RateLimiter } from '#lib/server/rate-limit.ts';
import { createTestServices, ROOT } from '#lib/server/testing/core.ts';
import { callRoute, loadRoute } from '#lib/testing/route-event.ts';
import type { DmAvailability, NotificationChannel } from '#lib/types.ts';
import { actions, load } from './+page.server.ts';
import type { PageServerData } from './$types';

const DJ = 'did:plc:dj';

/** A grant that lets the app read the account's email address. */
const WITH_EMAIL = 'atproto account:email?action=read';

let t: Awaited<ReturnType<typeof createTestServices>>;

let channels: NotificationChannels;

let outbox: Outbox;

/** Each confirmation email the site sent: where it went and the link's token. */
let links: { address: string; token: string }[];

/** How the next test message by each channel goes. */
let outcomes: Record<NotificationChannel, SendOutcome>;

/** Whether Bluesky chat says the root's DMs reach the viewer. */
let dmAvailability: DmAvailability;

/** The settings page's own limits: tries at turning a channel on, and test messages. */
let limits: ReturnType<typeof pageLimits>;

/** What the account's host says of its email when asked, or null when it fails. */
let hostEmail: SessionEmail | null;

/** Whether the viewer has an OAuth session the site can use. */
let restorable: boolean;

/** What asking the account's host for the address throws, if anything. */
let authorizeFails: Error | null;

/** The settings page's limits as the app sets them: five tries at turning a channel on in ten minutes, three tests an hour. */
const pageLimits = () => ({ optIn: new RateLimiter(5, 5 / 600), testSend: new RateLimiter(3, 3 / 3600) });

/** The site's limits on confirmation emails, roomy enough that no test here reaches them. */
const confirmLimits = (): ConfirmLimits => ({
	account: new RateLimiter(5, 5 / 86_400),
	accountAddress: new RateLimiter(3, 3 / 86_400),
	address: new RateLimiter(10, 10 / 86_400),
	all: new RateLimiter(100, 100 / 3600)
});

/** A channel's sender that answers with whatever `outcomes` says for it. */
const sender = (channel: NotificationChannel): ChannelSender => ({
	windowMs: 60_000,
	send: async () => outcomes[channel]
});

beforeEach(async () => {
	t = await createTestServices();
	links = [];
	outcomes = { email: { kind: 'sent', providerId: 'email-1' }, dm: { kind: 'sent', providerId: null } };
	dmAvailability = 'ok';
	hostEmail = { email: 'dj@neontide.test', emailConfirmed: true };
	restorable = true;
	authorizeFails = null;
	limits = pageLimits();

	channels = new NotificationChannels({
		db: t.db,
		logger: silentLogger,
		sendConfirmation: async (address, token) => {
			links.push({ address, token });
		},
		limits: confirmLimits()
	});

	outbox = new Outbox({
		db: t.db,
		logger: silentLogger,
		channels,
		senders: { email: sender('email'), dm: sender('dm') }
	});
});

afterEach(async () => {
	outbox.stop();
	await t.audit.idle();
	await t.db.destroy();
});

/** The account's OAuth session, which answers getSession with what `hostEmail` says. */
const session = (did: string) => ({
	did,
	fetchHandler: async () =>
		hostEmail
			? Response.json({ did, handle: 'dj.test', ...hostEmail })
			: Response.json({ error: 'InternalServerError', message: 'down' }, { status: 500 })
});

/** The site's OAuth client: sessions as `restorable` says, and the host's sign-in page carrying the request's state. */
const oauth = {
	restore: async (did: string) => (restorable ? session(did) : null),
	authorize: async (did: string, state: string, options: { optional?: string }) => {
		if (authorizeFails) throw authorizeFails;

		return new URL(
			`https://pds.test/oauth/authorize?${new URLSearchParams({ did, state, scope: options.optional ?? '' })}`
		);
	}
};

/** The services the settings page reads beyond the test services that a test swaps for a site set up differently. */
type Swappable = {
	notificationChannels: NotificationChannels;
	outbox: Outbox;
	dm: Pick<DmSender, 'availability'> | null;
	rootAccount: { handle: string | null };
};

/** The app the settings page reads, with each service it uses beyond the test services. */
const app = (overrides: Partial<Swappable> = {}) => ({
	...t,
	notificationChannels: channels,
	outbox,
	dm: { availability: async () => dmAvailability },
	rootAccount: { handle: 'localnetwork.test' },
	rootDid: ROOT,
	limits,
	oauth,
	...overrides
});

/** Records the DJ's sign-in with `scope`. */
const signedIn = (scope: string) =>
	t.accounts.recordLogin({
		did: DJ,
		handle: 'dj.test',
		pdsUrl: 'https://pds.test',
		grantedScope: scope,
		spacesCapable: false,
		publicWrites: false
	});

type ActionName = keyof typeof actions;

/** Runs the settings action `name` as the DJ, with `form`, on `site` (the default app unless given). */
const run = async (name: ActionName, form: Record<string, string | string[]> = {}, site = app()) =>
	callRoute(actions[name], {
		app: site,
		viewer: await t.viewer(DJ),
		url: `/dashboard/account/notifications?/${name}`,
		form
	});

it('shows both channels as off and available, with the account to follow for DMs', async () => {
	const data = await loadRoute<PageServerData>(load, {
		app: app(),
		viewer: await t.viewer(DJ),
		url: '/dashboard/account/notifications'
	});

	expect(data).toMatchObject({
		// Only the topics something is sent about.
		topics: ['bookings'],
		email: { available: true, channel: null, returned: null },
		dm: { available: true, channel: null, root: 'localnetwork.test' }
	});
	expect(await data.dm.availability).toBe('ok');
});

it("says how asking for the address went, and what's off on a site without email or DMs", async () => {
	const bare = app({
		outbox: new Outbox({ db: t.db, logger: silentLogger, channels, senders: {} }),
		dm: null,
		rootAccount: { handle: null }
	});

	await channels.enableDm(DJ, false);
	const viewer = await t.viewer(DJ);

	const declined = await loadRoute<PageServerData>(load, {
		app: bare,
		viewer,
		url: '/dashboard/account/notifications?email=declined'
	});

	expect(declined.email).toMatchObject({ available: false, returned: 'declined' });
	expect(declined.dm).toMatchObject({
		available: false,
		channel: { status: 'paused', pausedReason: 'dm_not_allowed', address: null },
		availability: null,
		root: ROOT
	});

	const limited = await loadRoute<PageServerData>(load, {
		app: bare,
		viewer,
		url: '/dashboard/account/notifications?email=limited'
	});

	const unknown = await loadRoute<PageServerData>(load, {
		app: bare,
		viewer,
		url: '/dashboard/account/notifications?email=yes'
	});

	expect([limited.email.returned, unknown.email.returned]).toEqual(['limited', null]);
});

it('sends someone signed out to sign in', async () => {
	await expect(callRoute(load, { app: app(), url: '/dashboard/account/notifications' })).rejects.toMatchObject({
		status: 303,
		location: '/login?next=%2Fdashboard%2Faccount%2Fnotifications'
	});
});

it('reads a new address with the session the viewer has, and emails it a link to confirm', async () => {
	await signedIn(WITH_EMAIL);

	expect(await run('emailOn')).toEqual({
		channel: 'email',
		success: 'We emailed you a link. Open it to turn email on.'
	});
	expect(links.map((link) => link.address)).toEqual(['dj@neontide.test']);
	expect(await channels.view(DJ, 'email')).toMatchObject({ status: 'paused', pausedReason: 'email_unverified' });
});

it("turns email back on for an address already confirmed here, and says when the host hasn't confirmed one", async () => {
	await signedIn(WITH_EMAIL);
	await run('emailOn');
	await channels.confirmEmail(links[0].token);
	await run('emailOff');

	// Off forgets the address, so it's confirmed again first; once it is, turning it on again needs no new link.
	await run('emailOn');
	await channels.confirmEmail(links[1].token);

	expect(await run('emailOn')).toEqual({ channel: 'email', success: "Email's on." });
	expect(links).toHaveLength(2);

	hostEmail = { email: 'new@neontide.test', emailConfirmed: false };

	expect(await run('emailOn')).toEqual({ channel: 'email', success: 'Checked your address.' });
	expect(await channels.view(DJ, 'email')).toMatchObject({ status: 'paused', pausedReason: 'email_unconfirmed' });
});

it("says it can't send a confirmation link when the site won't send one now", async () => {
	await signedIn(WITH_EMAIL);
	const noEmail = new NotificationChannels({ db: t.db, logger: silentLogger, limits: confirmLimits() });

	const result = await run('emailOn', {}, app({ notificationChannels: noEmail }));

	expect(result).toMatchObject({
		status: 429,
		data: {
			channel: 'email',
			error: "We can't send a confirmation link to that address right now. Try again tomorrow."
		}
	});
});

it("asks the account's host for the address when the session can't read it, then comes back to the settings", async () => {
	const state = JSON.stringify({ next: '/dashboard/account/notifications', intent: 'email' });
	const expected = { status: 303, location: expect.stringContaining(new URLSearchParams({ state }).toString()) };

	// No account recorded, so no grant to read it with.
	await expect(run('emailOn')).rejects.toMatchObject(expected);

	// A grant without email.
	await signedIn('atproto');
	await expect(run('emailOn')).rejects.toMatchObject(expected);

	// A grant with email, but no session left to use it.
	await signedIn(WITH_EMAIL);
	restorable = false;
	await expect(run('emailOn')).rejects.toMatchObject(expected);

	// A session whose host won't answer.
	restorable = true;
	hostEmail = null;
	await expect(run('emailOn')).rejects.toMatchObject({
		location: expect.stringMatching(/^https:\/\/pds\.test\/oauth\/authorize\?did=did%3Aplc%3Adj&.*&scope=email$/)
	});
	expect(await channels.view(DJ, 'email')).toBeNull();
});

it("says when the account's host can't share the address, or can't be reached", async () => {
	authorizeFails = new OptionalScopeUnsupported('email');

	expect(await run('emailOn')).toMatchObject({
		status: 400,
		data: { channel: 'email', error: "Your account's server can't share your email address with apps." }
	});

	authorizeFails = new Error('connect ECONNREFUSED');

	expect(await run('emailOn')).toMatchObject({
		status: 502,
		data: { channel: 'email', error: "We couldn't reach your account's server. Try again in a minute." }
	});
});

it('stops someone trying to turn email or DMs on more than five times in a few minutes', async () => {
	const site = app({ dm: { availability: async () => 'unavailable' } });

	for (let i = 0; i < 5; i++) await expect(run('emailOn', {}, site)).rejects.toMatchObject({ status: 303 });

	expect(await run('emailOn', {}, site)).toMatchObject({ status: 429, data: { channel: 'email' } });
	expect(await run('dmOn', {}, site)).toMatchObject({
		status: 429,
		data: { channel: 'dm', error: 'Too many tries. Wait a few minutes, then try again.' }
	});
});

it('sends a channel only the topics picked, and says when none are', async () => {
	await channels.enableDm(DJ, true);

	expect(await run('dmTopics', { topic: ['bookings', 'spam'] })).toEqual({ channel: 'dm', success: 'Saved.' });
	expect((await channels.view(DJ, 'dm'))?.topics).toEqual(['bookings']);

	expect(await run('emailTopics')).toEqual({
		channel: 'email',
		success: 'Saved. Nothing will be emailed until you pick something.'
	});
	expect(await run('dmTopics')).toEqual({
		channel: 'dm',
		success: "Saved. Nothing will be DM'd until you pick something."
	});
	// A topic nothing is sent about yet isn't one to pick.
	expect(await run('emailTopics', { topic: 'guests' })).toEqual({
		channel: 'email',
		success: 'Saved. Nothing will be emailed until you pick something.'
	});
	expect(await run('emailTopics', { topic: 'bookings' })).toEqual({ channel: 'email', success: 'Saved.' });
	expect((await channels.view(DJ, 'dm'))?.topics).toEqual([]);
});

it('sends a test message by a channel that is on', async () => {
	await channels.enableDm(DJ, true);

	expect(await run('dmTest')).toEqual({ channel: 'dm', success: 'Sent. Check your Bluesky chats.' });
});

it("won't test a channel that's off, and stops after three tests an hour", async () => {
	await channels.enableDm(DJ, true);

	expect(await run('emailTest')).toMatchObject({ status: 400, data: { channel: 'email', error: 'Turn it on first.' } });
	await run('dmTest');
	await run('dmTest');

	expect(await run('dmTest')).toMatchObject({
		status: 429,
		data: { channel: 'dm', error: "That's enough tests for now. Try again in an hour." }
	});
});

it('pauses a channel whose test is refused, and says when one could not be sent', async () => {
	await channels.enableDm(DJ, true);
	outcomes.dm = { kind: 'failed', error: 'chat service misconfigured' };

	expect(await run('dmTest')).toMatchObject({
		status: 502,
		data: { channel: 'dm', error: "We couldn't send it. Try again later." }
	});

	outcomes.dm = { kind: 'rejected', reason: 'dm_not_allowed', error: 'recipient blocks DMs' };

	expect(await run('dmTest')).toMatchObject({
		status: 400,
		data: { channel: 'dm', error: "It didn't go through, so it's paused." }
	});
	expect(await channels.view(DJ, 'dm')).toMatchObject({ status: 'paused', pausedReason: 'dm_not_allowed' });
});

it('turns each channel off', async () => {
	await signedIn(WITH_EMAIL);
	await run('emailOn');
	await channels.enableDm(DJ, true);

	expect(await run('emailOff')).toEqual({ channel: 'email', success: "Email's off. We've deleted your address." });
	expect(await run('dmOff')).toEqual({ channel: 'dm', success: 'DMs are off.' });
	expect([await channels.view(DJ, 'email'), await channels.view(DJ, 'dm')]).toEqual([null, null]);
});

it('turns DMs on when Bluesky chat lets them through', async () => {
	expect(await run('dmOn')).toEqual({ channel: 'dm', success: 'DMs are on.' });
	expect(await channels.view(DJ, 'dm')).toMatchObject({ status: 'active', pausedReason: null });
});

it("holds DMs while Bluesky chat won't deliver them, and changes nothing when chat can't be reached", async () => {
	dmAvailability = 'unavailable';

	expect(await run('dmOn')).toMatchObject({
		status: 502,
		data: { channel: 'dm', error: "We couldn't reach Bluesky chat. Try again in a minute." }
	});
	expect(await channels.view(DJ, 'dm')).toBeNull();

	dmAvailability = 'not_allowed';

	expect(await run('dmOn')).toMatchObject({
		status: 400,
		data: { channel: 'dm', error: "Bluesky chat won't deliver our DMs to you yet." }
	});
	expect(await channels.view(DJ, 'dm')).toMatchObject({ status: 'paused', pausedReason: 'dm_not_allowed' });
});

it("says DMs aren't set up on a site without them", async () => {
	expect(await run('dmOn', {}, app({ dm: null }))).toMatchObject({
		status: 400,
		data: { channel: 'dm', error: "Bluesky DMs aren't set up on this site." }
	});
});
