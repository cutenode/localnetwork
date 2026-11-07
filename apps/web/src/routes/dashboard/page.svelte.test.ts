import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { NeedsRow, NightRow } from '#lib/home.ts';
import type { OnboardingData } from '#lib/onboarding/checklist.ts';
import type { ProjectView, Viewer } from '#lib/types.ts';
import Page from './+page.svelte';
import type { PageProps } from './$types';

const viewer = (spacesCapable = true): Viewer => ({
	did: 'did:plc:neon',
	handle: 'neon.test',
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable
});

const need = (title: string, action = 'Reply'): NeedsRow => ({
	key: title,
	scope: 'Neon Tide',
	title,
	detail: 'Sat 14 Nov · From @pat.test · 2 hours ago',
	href: `/dashboard/bookings/${title}`,
	action
});

const night = (title: string, overrides: Partial<NightRow> = {}): NightRow => ({
	key: title,
	date: '2026-11-14',
	title,
	detail: 'You play 23:00 as Neon Tide',
	href: `/dashboard/shows/${title}`,
	tentative: false,
	...overrides
});

const nights = (rows: NightRow[], overrides = {}) =>
	Promise.resolve({ nights: rows, more: 0, failed: false, ...overrides });

// SAFETY: the page reads only an act's slug and name.
const act = { slug: 'neon-tide', profile: { name: 'Saffron Social' } } as ProjectView;

const base: PageProps['data'] = {
	viewer: viewer(),
	pendingRequests: 0,
	unread: 0,
	sections: Promise.resolve(null),
	projects: Promise.resolve([act]),
	waiting: 0,
	needs: [],
	access: Promise.resolve([]),
	firstRun: null,
	summary: { others: null, hasBookings: true },
	nights: nights([]),
	onboarding: null
};

const open = (data: Partial<PageProps['data']>) => render(Page, { params: {}, form: null, data: { ...base, ...data } });

it('says how many things need the viewer and their next night, then lists both', async () => {
	await open({
		waiting: 2,
		needs: [need('Reply to Rooftop Rave'), need('Reply to Closing Night')],
		nights: nights([night('Friday Social', { when: 'Tomorrow' }), night('Late Room', { tentative: true })])
	});

	await expect.element(page.getByRole('heading', { level: 1 })).toHaveTextContent('2 things need you.');
	await expect.element(page.getByText('Your next night is tomorrow: Friday Social.')).toBeInTheDocument();

	const needs = page.getByRole('region', { name: 'Needs you' });
	await expect
		.element(needs.getByRole('link', { name: 'Reply: Reply to Rooftop Rave' }))
		.toHaveAttribute('href', '/dashboard/bookings/Reply to Rooftop Rave');
	// Only the first row is the page's primary action.
	expect(
		needs
			.getByRole('link')
			.elements()
			.map((link) => link.classList.contains('btn-primary'))
	).toEqual([true, false]);

	await expect
		.element(page.getByRole('region', { name: 'Next up' }).getByRole('link', { name: 'Friday Social' }))
		.toHaveAttribute('href', '/dashboard/shows/Friday Social');
	await expect.element(page.getByText('Tomorrow', { exact: true })).toBeInTheDocument();
});

it('names a night by its date when it is further off', async () => {
	await open({ nights: nights([night('Late Room', { date: '2026-11-14' })]) });

	await expect.element(page.getByText('Your next night is Sat 14 Nov: Late Room.')).toBeInTheDocument();
});

it('adds people waiting on a press kit to the count once they arrive', async () => {
	await open({
		waiting: 1,
		needs: [need('Reply to Rooftop Rave')],
		access: Promise.resolve([need('Let @lena.test see Clubs?', 'Review')])
	});

	await expect.element(page.getByRole('heading', { level: 1 })).toHaveTextContent('2 things need you.');
	await expect.element(page.getByRole('link', { name: 'Review: Let @lena.test see Clubs?' })).toBeInTheDocument();
});

it('stops the list at five and says how many more', async () => {
	await open({ waiting: 7, needs: ['a', 'b', 'c', 'd', 'e', 'f', 'g'].map((title) => need(title)) });

	await expect.element(page.getByRole('heading', { level: 1 })).toHaveTextContent('7 things need you.');
	expect(page.getByRole('region', { name: 'Needs you' }).getByRole('listitem').elements()).toHaveLength(5);
	await expect.element(page.getByRole('link', { name: '2 more' })).toHaveAttribute('href', '/dashboard/bookings');
});

it('says when more nights follow', async () => {
	await open({ nights: nights([night('Late Room')], { more: 2 }) });

	await expect.element(page.getByText('And 2 more nights after that.')).toBeInTheDocument();
});

it('drops empty sections, leaving the status line and what others have', async () => {
	await open({
		summary: { others: '1 booking is waiting on a promoter. Nothing needed from you.', hasBookings: true }
	});

	await expect.element(page.getByRole('heading', { level: 1 })).toHaveTextContent('Nothing needs you.');
	await expect.element(page.getByText('No nights coming up.')).toBeInTheDocument();
	await expect.element(page.getByText('1 booking is waiting on a promoter.', { exact: false })).toBeInTheDocument();
	await expect.element(page.getByRole('region', { name: 'Needs you' })).not.toBeInTheDocument();
	await expect.element(page.getByRole('region', { name: 'Next up' })).not.toBeInTheDocument();
	await expect.element(page.getByText('Nothing here yet')).not.toBeInTheDocument();
});

it('holds room for what needs the viewer while it streams, and shows it when it lands', async () => {
	let land = (_rows: NeedsRow[]) => {};

	const needs = new Promise<NeedsRow[]>((resolve) => (land = resolve));

	await open({ waiting: 1, needs });

	await expect.element(page.getByRole('heading', { level: 1 })).toHaveTextContent('1 thing needs you.');
	const room = page.getByRole('status', { name: 'Loading what needs you' });
	expect(room.getByRole('listitem').elements()).toHaveLength(1);

	land([need('Reply to Rooftop Rave')]);

	await expect.element(page.getByRole('link', { name: 'Reply: Reply to Rooftop Rave' })).toBeInTheDocument();
	await expect.element(page.getByRole('status', { name: 'Loading what needs you' })).not.toBeInTheDocument();
});

it('asks a streamed first visit to set up once it knows, in place of the status', async () => {
	await open({
		firstRun: Promise.resolve({ name: 'Pat', hasAvatar: false }),
		summary: Promise.resolve({ others: null, hasBookings: false })
	});

	await expect.element(page.getByRole('heading', { level: 1 })).toHaveTextContent('Set up your page');
});

it('holds room for nights still on their way, without holding up what needs the viewer', async () => {
	await open({ waiting: 1, needs: [need('Reply to Rooftop Rave')], nights: new Promise(() => {}) });

	await expect.element(page.getByRole('link', { name: 'Reply: Reply to Rooftop Rave' })).toBeInTheDocument();
	await expect.element(page.getByRole('status', { name: 'Loading your nights' })).toBeInTheDocument();
	expect(document.querySelector('[aria-busy="true"]')?.querySelectorAll('li')).toHaveLength(3);
	await expect.element(page.getByText('Finding your next night')).toBeInTheDocument();
});

it('keeps the nights it has and offers another try when some are missing', async () => {
	await open({ nights: nights([night('Rooftop Rave')], { failed: true }) });

	await expect.element(page.getByRole('link', { name: 'Rooftop Rave' })).toBeInTheDocument();
	await expect.element(page.getByText("Some of your nights didn't load.", { exact: false })).toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Try again' })).toHaveAttribute('href', '/dashboard');
});

it('offers another try, not a broken page, when the nights fail outright', async () => {
	await open({ nights: Promise.reject(new Error('down')) });

	await expect.element(page.getByRole('link', { name: 'Try again' })).toBeInTheDocument();
});

const fresh: OnboardingData = {
	profile: null,
	prefill: { name: 'Pat', hasAvatar: false },
	listing: false,
	follows: false
};

/** The steps the checklist lists, with an x on each one done. */
const steps = () =>
	page
		.getByRole('region', { name: 'Your checklist' })
		.getByRole('listitem')
		.elements()
		.map(
			(item) => `${item.querySelector('[aria-label="Done"]') ? 'x' : ' '} ${item.querySelector('a, p')?.textContent}`
		);

it('leads a first visit with the checklist, making the profile right there', async () => {
	await open({
		firstRun: { name: 'Pat', hasAvatar: false },
		summary: { others: null, hasBookings: false },
		projects: Promise.resolve([]),
		onboarding: Promise.resolve(fresh)
	});

	await expect.element(page.getByRole('heading', { level: 1 })).toHaveTextContent('Set up your page');
	await expect.element(page.getByText("This goes away when you're done.")).toBeInTheDocument();
	await expect.element(page.getByText('0 of 6 done')).toBeInTheDocument();
	await expect.element(page.getByLabelText('Name')).toHaveValue('Pat');
	await expect
		.element(page.getByRole('link', { name: 'Make your profile' }))
		.toHaveAttribute('href', '/dashboard/profile');
	expect(steps()).toEqual([
		'  Make your profile',
		'  Add a picture',
		'  Set your home city',
		'  Add a bio and links',
		'  Say if you DJ or put on shows',
		'  Follow a DJ'
	]);
});

it('ticks off what a half-done profile has, and puts the checklist ahead of what waits', async () => {
	await open({
		waiting: 1,
		needs: [need('Answer Neon Tide about Rooftop Rave')],
		onboarding: {
			...fresh,
			prefill: null,
			profile: { basics: { name: 'Pat', bio: 'Hi' }, performer: {}, createdAt: '', updatedAt: '' },
			follows: true
		}
	});

	await expect.element(page.getByText('3 of 7 done')).toBeInTheDocument();
	expect(steps()).toEqual([
		'x Make your profile',
		'  Add a picture',
		'  Set your home city',
		'  Add a bio and links',
		'x Say if you DJ or put on shows',
		'  Ask to be listed in Find DJs',
		'x Follow a DJ'
	]);
	await expect.element(page.getByLabelText('Name')).not.toBeInTheDocument();
	await expect
		.element(page.getByRole('link', { name: 'Ask to be listed in Find DJs' }))
		.toHaveAttribute('href', '/dashboard/profile#listing');

	const [first, second] = [
		page.getByRole('region', { name: 'Your checklist' }).element(),
		page.getByRole('region', { name: 'Needs you' }).element()
	];

	expect(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});

it('drops the checklist once every step is done', async () => {
	const profile = {
		basics: {
			name: 'Pat',
			bio: 'Parties since 2019.',
			links: [{ kind: 'website', url: 'https://pat.test' }],
			location: { city: 'Berlin', country: 'DE' },
			avatar: { $type: 'blob' as const, ref: { $link: 'bafy' }, mimeType: 'image/png', size: 1 }
		},
		promoter: {},
		createdAt: '',
		updatedAt: ''
	};

	await open({ onboarding: { ...fresh, prefill: null, profile, follows: true } });

	await expect.element(page.getByRole('heading', { level: 1 })).toBeInTheDocument();
	await expect.element(page.getByRole('region', { name: 'Your checklist' })).not.toBeInTheDocument();
});

it('suggests freshening up an act when all is quiet', async () => {
	await open({
		summary: { others: null, hasBookings: false },
		projects: Promise.resolve([{ ...act, profile: { ...act.profile, name: 'Neon Tide' } }])
	});

	await expect
		.element(page.getByRole('link', { name: 'Neon Tide' }))
		.toHaveAttribute('href', '/dashboard/projects/neon-tide');
});

it('suggests freshening up an act when all is quiet', async () => {
	const neon = { ...act, profile: { ...act.profile, name: 'Neon Tide' } };

	await open({ summary: { others: null, hasBookings: false }, projects: Promise.resolve([neon]) });

	await expect
		.element(page.getByRole('link', { name: 'Neon Tide' }))
		.toHaveAttribute('href', '/dashboard/projects/neon-tide');
});

it('gives someone with nothing yet a next step', async () => {
	await open({ summary: { others: null, hasBookings: false }, projects: Promise.resolve([]) });

	await expect.element(page.getByText('Nothing here yet')).toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Find DJs' })).toHaveAttribute('href', '/projects');
	await expect
		.element(page.getByRole('link', { name: 'List a project' }))
		.toHaveAttribute('href', '/dashboard/projects/new');
});

it("says when the account's server can't keep private data", async () => {
	await open({ viewer: viewer(false) });

	await expect
		.element(page.getByRole('link', { name: 'see what it keeps' }))
		.toHaveAttribute('href', '/dashboard/account/data');
});
