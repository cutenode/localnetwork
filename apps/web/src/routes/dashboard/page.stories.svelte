<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { BookingThreadView, PlannerShowView, ProjectView, Viewer } from '#lib/types.ts';
	import Page from './+page.svelte';

	/** The dashboard overview: a promoter's leads with their threads and planner, a DJ's with their projects. */
	const { Story } = defineMeta({
		title: 'Patterns/Dashboard overview',
		parameters: { sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/dashboard') } } } }
	});

	const viewer = (did: string, handle: string): Viewer => ({ did, handle, roles: [], isAdmin: false, isStaff: false, spacesCapable: true });

	const neon: ProjectView = {
		uri: 'at://did:plc:root/computer.localnetwork.project/2',
		rkey: '2',
		cid: 'c',
		slug: 'neon-tide',
		profile: { name: 'Neon Tide', kind: 'solo', genres: ['techno'], location: { city: 'Berlin', country: 'DE' } },
		managers: ['did:plc:neon'],
		status: 'active',
		revision: 1,
		createdBy: 'did:plc:a',
		createdAt: '2026-01-01T00:00:00Z',
		updatedAt: '2026-01-01T00:00:00Z',
		avatarUrl: null
	};

	const thread = (skey: string, title: string, status: string, lastAuthor: string): BookingThreadView => ({
		space: `at://did:plc:root/computer.localnetwork.space.booking/${skey}`,
		skey,
		status,
		title,
		showDate: null,
		project: { uri: neon.uri, slug: 'neon-tide', name: 'Neon Tide' },
		promoter: { did: 'did:plc:pat', handle: 'pat.test' },
		participants: [
			{ did: 'did:plc:pat', handle: 'pat.test', role: 'promoter' },
			{ did: 'did:plc:neon', handle: 'neon.test', role: 'manager' }
		],
		lastAuthor,
		lastActivityAt: '2026-10-30T20:00:00Z',
		createdAt: '2026-10-28T20:00:00Z'
	});

	const threads = [thread('3k1', 'Closing Night', 'negotiating', 'did:plc:neon'), thread('3k2', 'Rooftop Rave', 'open', 'did:plc:pat')];

	const show: PlannerShowView = {
		rkey: 's1',
		details: { title: 'Winter Social', venue: 'Kantine', date: '2026-12-12T22:00:00.000Z' },
		lineup: [{ project: neon.uri }],
		createdAt: '2026-10-01T00:00:00Z'
	};

	const base = { pendingRequests: 0, unread: 0, sections: Promise.resolve(null), requests: [], threads: [], waiting: 0, firstRun: null };

	const promoter = { ...base, viewer: viewer('did:plc:pat', 'pat.test'), projects: Promise.resolve([]), books: Promise.resolve(true) };

	const planner = { available: true as const, saves: [{ rkey: 'a', project: neon.uri, createdAt: '' }], shows: [show], projects: { [neon.uri]: neon } };

	const dj = { ...base, viewer: viewer('did:plc:neon', 'neon.test'), books: Promise.resolve(false), planner: Promise.resolve(null) };

	const loading = new Promise<never>(() => {});

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Promoter" asChild>
	<Page params={{}} form={null} data={{ ...promoter, threads, waiting: 1, planner: Promise.resolve(planner) }} />
</Story>

<Story name="Promoter on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={{ ...promoter, threads, waiting: 1, planner: Promise.resolve(planner) }} />
</Story>

<!-- A promoter's first visit: nothing planned, saved or sent yet, each with its next step. -->
<Story name="Promoter, empty" asChild>
	<Page params={{}} form={null} data={{ ...promoter, planner: Promise.resolve({ available: true as const, saves: [], shows: [], projects: {} }), firstRun: { name: 'Pat', hasAvatar: false } }} />
</Story>

<Story name="Promoter, planner didn't load" asChild>
	<Page params={{}} form={null} data={{ ...promoter, threads, planner: Promise.resolve({ available: false as const, saves: [], shows: [], projects: {}, problem: 'Your PDS did not respond' }) }} />
</Story>

<Story name="Loading" asChild>
	<Page params={{}} form={null} data={{ ...promoter, projects: loading, books: loading, planner: loading }} />
</Story>

<Story name="DJ" asChild>
	<Page params={{}} form={null} data={{ ...dj, projects: Promise.resolve([neon]), threads: threads.toReversed(), waiting: 1 }} />
</Story>

<Story name="DJ on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={{ ...dj, projects: Promise.resolve([neon]), threads: threads.toReversed(), waiting: 1 }} />
</Story>

<!-- A DJ profile that hasn't listed or claimed a project yet. -->
<Story name="DJ, no projects" asChild>
	<Page params={{}} form={null} data={{ ...dj, projects: Promise.resolve([]) }} />
</Story>

<Story name="Directory offline" asChild>
	<Page params={{}} form={null} data={{ ...dj, projects: Promise.resolve(null) }} />
</Story>
