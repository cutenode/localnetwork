import { page } from 'vitest/browser';
import { expect, it } from 'vitest';
import { render } from 'vitest-browser-svelte';
import type { BookingListThread, ProjectView, ThreadProjects, Viewer } from '#lib/types.ts';
import Page from './+page.svelte';
import type { PageProps } from './$types';

const viewer = (did: string): Viewer => ({
	did,
	handle: null,
	roles: [],
	isAdmin: false,
	isStaff: false,
	spacesCapable: true
});

const PAT = 'did:plc:pat';

const NEON = 'did:plc:neon';

const thread = (
	skey: string,
	status: string,
	lastAuthor: string,
	overrides: Partial<BookingListThread> = {}
): BookingListThread => ({
	space: `at://did:plc:root/computer.localnetwork.space.booking/${skey}`,
	skey,
	status,
	title: `Show ${skey}`,
	showDate: null,
	project: null,
	promoter: { did: PAT, handle: 'pat.test' },
	participants: [
		{ did: PAT, handle: 'pat.test', role: 'promoter' },
		{ did: NEON, handle: 'neon.test', role: 'manager' }
	],
	lastAuthor,
	lastActivityAt: '2026-10-01T00:00:00Z',
	createdAt: '2026-10-01T00:00:00Z',
	withdrawn: false,
	...overrides
});

/** Which act each thread is with: Neon Tide, unless its key is listed under Harbour Lights. */
const named = (threads: BookingListThread[], harbour: string[] = []): ThreadProjects =>
	new Map(
		threads.map(({ space, skey }) => {
			const slug = harbour.includes(skey) ? 'harbour-lights' : 'neon-tide';

			return [
				space,
				{
					uri: `at://did:plc:root/computer.localnetwork.project/${slug}`,
					slug,
					name: harbour.includes(skey) ? 'Harbour Lights' : 'Neon Tide'
				}
			];
		})
	);

const data = (
	did: string,
	threads: BookingListThread[],
	overrides: Partial<PageProps['data']> = {}
): PageProps['data'] => ({
	viewer: viewer(did),
	pendingRequests: 0,
	unread: 0,
	waiting: 0,
	needs: [],
	access: Promise.resolve([]),
	sections: Promise.resolve(null),
	projects: Promise.resolve([]),
	threads,
	acts: new Set(threads.map(({ skey }) => skey)).size,
	names: named(threads),
	filter: null,
	...overrides
});

const list = (props: PageProps['data']) => render(Page, { params: {}, form: null, data: props });

const region = (name: string) => page.getByRole('region', { name: new RegExp(`^${name}`) });

const text = (name: string) => region(name).element().textContent.replaceAll(/\s+/g, ' ');

it('groups threads by whose turn it is, with a filter once there are several acts', async () => {
	const threads = [
		thread('a', 'negotiating', NEON),
		thread('b', 'open', PAT),
		thread('c', 'confirmed', NEON),
		thread('d', 'declined', NEON)
	];

	await list(data(PAT, threads, { names: named(threads, ['b']) }));

	for (const [group, show] of [
		['Your turn', 'Show a'],
		['Waiting on them', 'Show b'],
		['Confirmed', 'Show c'],
		['Closed', 'Show d']
	]) {
		await expect.element(region(group).getByText(show)).toBeInTheDocument();
	}

	await expect.element(page.getByText('In progress')).not.toBeInTheDocument();

	const filter = page.getByRole('navigation', { name: 'Filter by act' });
	await expect.element(filter.getByRole('link', { name: 'All' })).toHaveAttribute('aria-current', 'page');
	await expect
		.element(filter.getByRole('link', { name: 'Harbour Lights' }))
		.toHaveAttribute('href', '/dashboard/bookings?project=harbour-lights');
});

it('says who a booking waits on: the act to a promoter, the handle to a manager', async () => {
	const threads = [thread('a', 'open', PAT)];
	const { rerender } = await list(data(PAT, threads));

	expect(text('Waiting on them')).toContain('Waiting on Neon Tide · you wrote');
	expect(text('Waiting on them')).not.toContain('neon.test');

	const answered = [thread('a', 'negotiating', NEON)];
	await rerender({ data: data(NEON, answered) });
	await expect.poll(() => text('Waiting on them')).toContain('Waiting on @pat.test · Neon Tide · you wrote');
});

it('leads a row with its date, dashed until confirmed, and marks a date with none', async () => {
	await list(
		data(NEON, [
			thread('a', 'open', PAT, { showDate: '2099-11-14' }),
			thread('b', 'confirmed', PAT, { showDate: '2099-11-21' }),
			thread('c', 'open', PAT)
		])
	);

	const dashed = (show: string) =>
		page.getByRole('listitem').filter({ hasText: show }).element().querySelector('time')?.className;

	expect(dashed('Show a')).toMatch(/border-dashed/);
	expect(dashed('Show b')).not.toMatch(/border-dashed/);
	await expect.element(page.getByRole('listitem').filter({ hasText: 'Show c' }).getByText('No date')).toBeVisible();
});

it('shows a pill only where it adds to the group', async () => {
	await list(
		data(NEON, [
			thread('a', 'open', PAT),
			thread('b', 'negotiating', PAT),
			thread('c', 'open', NEON),
			thread('d', 'confirmed', PAT, { showDate: '2099-11-21' }),
			thread('e', 'negotiating', PAT, { showDate: '2020-11-21' })
		])
	);

	const row = (show: string) => page.getByRole('listitem').filter({ hasText: show });

	await expect.element(row('Show a').getByText('New', { exact: true })).toBeVisible();
	await expect.element(row('Show b').getByText('Negotiating', { exact: true })).toBeVisible();
	await expect.element(row('Show c').getByText('New', { exact: true })).not.toBeInTheDocument();
	await expect.element(row('Show d').getByText('Confirmed', { exact: true })).not.toBeInTheDocument();
	await expect.element(row('Show e').getByText('Date passed')).toBeVisible();
});

it('puts Closed away, each row saying how it ended', async () => {
	await list(
		data(PAT, [
			thread('a', 'declined', NEON),
			thread('b', 'cancelled', PAT, { withdrawn: true }),
			thread('c', 'cancelled', NEON),
			thread('d', 'confirmed', NEON, { showDate: '2020-11-21' })
		])
	);

	await expect.element(page.getByText('Show a')).not.toBeVisible();
	await page.getByText('Closed', { exact: false }).click();

	const row = (show: string) => page.getByRole('listitem').filter({ hasText: show });

	await expect.element(row('Show a').getByText('Declined')).toBeVisible();
	await expect.element(row('Show b').getByText('Withdrawn')).toBeVisible();
	await expect.element(row('Show c').getByText('Cancelled')).toBeVisible();
	await expect.element(row('Show d').getByText('Date passed')).toBeVisible();
});

it('renders every thread while the directory is slow, and fills the act in when it answers', async () => {
	const threads = [thread('a', 'open', NEON)];
	let answer = (_: ThreadProjects) => {};

	const names = new Promise<ThreadProjects>((resolve) => {
		answer = resolve;
	});

	await list(data(PAT, threads, { names }));

	await expect.element(page.getByText('Show a')).toBeVisible();
	expect(text('Your turn')).not.toContain('Neon Tide');
	answer(named(threads));
	await expect.poll(() => text('Your turn')).toContain('Neon Tide');
});

it('filters by the act in the address, and says when it has nothing', async () => {
	const threads = [thread('a', 'open', NEON), thread('b', 'open', NEON)];

	const filter = 'harbour-lights';
	const { rerender } = await list(data(PAT, threads, { filter, names: named(threads, ['b']) }));

	await expect.element(page.getByText('Show b')).toBeVisible();
	await expect.element(page.getByText('Show a')).not.toBeInTheDocument();
	await expect.element(page.getByRole('link', { name: 'Harbour Lights' })).toHaveAttribute('aria-current', 'page');

	await rerender({ data: data(PAT, threads, { filter }) });
	await expect.element(page.getByText('No bookings with this act')).toBeVisible();
	await expect.element(page.getByRole('link', { name: 'Show all' })).toHaveAttribute('href', '/dashboard/bookings');
});

it('holds the filter until the directory can say which threads match', async () => {
	const threads = [thread('a', 'open', NEON)];

	await list(data(PAT, threads, { acts: 2, filter: 'neon-tide', names: new Promise<never>(() => {}) }));

	await expect.element(page.getByRole('status', { name: 'Loading acts' })).toBeInTheDocument();
	await expect.element(page.getByRole('status', { name: 'Loading bookings' })).toBeInTheDocument();
	await expect.element(page.getByText('Show a')).not.toBeInTheDocument();
});

it('shows every booking when it has no names to filter by, and says so', async () => {
	await list(data(PAT, [thread('a', 'open', NEON)], { filter: 'neon-tide', names: null }));

	await expect.element(page.getByText('Show a')).toBeVisible();
	await expect.element(page.getByText(/so this shows every booking/)).toBeVisible();
});

it('works without act names, and says so', async () => {
	await list(data(PAT, [thread('a', 'open', PAT)], { names: null }));

	await expect.element(page.getByText(/Waiting on the act/)).toBeVisible();
	await expect.element(page.getByText(/Act names didn't load/)).toBeVisible();
	await expect.element(page.getByRole('link', { name: 'Try again' })).toBeVisible();
});

it('keeps New inquiry reachable', async () => {
	await list(data(PAT, [thread('a', 'open', PAT)]));

	await expect.element(page.getByRole('link', { name: 'New inquiry' })).toHaveAttribute('href', '/projects');
});

it('gives a DJ and a promoter each a next step when there are no bookings', async () => {
	// SAFETY: the empty state only reads the first project's slug.
	const projects = Promise.resolve([{ slug: 'neon-tide' } as ProjectView]);
	const { rerender } = await list(data(NEON, [], { projects }));

	await expect.element(page.getByRole('heading', { level: 1, name: 'Bookings' })).toBeVisible();
	await expect.element(page.getByText(/When a promoter sends an inquiry/)).toBeInTheDocument();
	await expect
		.element(page.getByRole('link', { name: 'Edit your page' }))
		.toHaveAttribute('href', '/dashboard/projects/neon-tide');
	await expect.element(page.getByRole('link', { name: 'Find DJs' })).not.toBeInTheDocument();
	await rerender({ data: data(PAT, []) });
	await expect.element(page.getByRole('link', { name: 'Find DJs' })).toHaveAttribute('href', '/projects');
});
