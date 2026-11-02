/**
 * What signing in lets Local Network do, in plain words, for "How signing in works". Each line names the collections,
 * spaces or `blob` it stands for (`covers`), and a test checks every scope the app may ask for
 * (src/lib/server/atproto/oauth.ts) is covered, so a new scope can't go unexplained.
 */
export const CAN: { text: string; covers: string[] }[] = [
	{
		text: 'Keep your profile and sets up to date',
		covers: ['computer.localnetwork.profile', 'computer.localnetwork.dj.set']
	},
	{
		text: 'Post your shows, RSVPs, follows and press kits',
		covers: [
			'computer.localnetwork.show',
			'community.lexicon.calendar.event',
			'community.lexicon.calendar.rsvp',
			'computer.localnetwork.follow',
			'computer.localnetwork.epk',
			'computer.localnetwork.space.epk'
		]
	},
	{ text: 'Upload photos and tracklists', covers: ['blob'] },
	{
		text: 'Keep your inquiries, requests, drafts and planner private',
		covers: [
			'computer.localnetwork.space.performer',
			'computer.localnetwork.space.booking',
			'computer.localnetwork.space.moderation',
			'computer.localnetwork.space.planner',
			'computer.localnetwork.space.backstage'
		]
	},
	{ text: 'Log the changes you make here', covers: ['computer.localnetwork.space.audit'] },
	{ text: 'Post a bug report, only when you send one', covers: ['app.userinput.discussion'] },
	{ text: 'See your email address, only if you turn on email', covers: ['account:email', 'transition:email'] }
];

/** What it never gets: none of the scopes reach the account itself or other apps' records. */
export const CANNOT = [
	'See your password',
	'Change your email, handle or password',
	'Delete your account',
	'Post, like or message on Bluesky'
];
