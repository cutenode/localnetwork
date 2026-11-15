<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { LocallibraryStatus } from '#lib/dj.ts';
	import type { NeedsRow, NightRow } from '#lib/home.ts';
	import type { OnboardingData } from '#lib/onboarding/checklist.ts';
	import type { ProjectView, Viewer } from '#lib/types.ts';
	import Page from './+page.svelte';

	/** Home: what needs the viewer and their next night, whatever they take part in. Each section arrives on its own. */
	const { Story } = defineMeta({
		title: 'Patterns/Dashboard home',
		parameters: { sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/dashboard') } } } }
	});

	const viewer = (spacesCapable = true): Viewer => ({ did: 'did:plc:neon', handle: 'neon.test', roles: [], isAdmin: false, isStaff: false, spacesCapable });

	const need = (key: string, scope: string, title: string, detail: string, action = 'Reply'): NeedsRow => ({ key, scope, title, detail, href: '/dashboard/bookings/3k1', action });

	const needs = [
		need('1', 'Neon Tide', 'Reply to Rooftop Rave', 'Sat 14 Nov · New inquiry from @pat.test · 2 hours ago'),
		need('2', 'Saffron Social', 'Reply to Walkthrough Night', 'No date yet · From @jonas.test · 3 hours ago'),
		need('3', 'Second Act', 'Update your request for Second Act', 'Staff asked for changes · yesterday', 'Update')
	];

	const access = [need('4', 'Neon Tide', 'Let @lena.test see Clubs?', 'Asked yesterday', 'Review')];

	const night = (key: string, date: string, title: string, detail: string, overrides: Partial<NightRow> = {}): NightRow => ({ key, date, title, detail, href: '/dashboard/planner', tentative: false, ...overrides });

	const nights = [
		night('a', '2026-11-07', 'Friday Social', 'You play 01:00 as Neon Tide · Tiefgarage', { when: 'Tomorrow' }),
		night('b', '2026-11-14', 'Rooftop Rave', 'Booking for Neon Tide · not confirmed', { tentative: true }),
		night('c', '2026-12-12', 'Winter Social', 'Planned · Halle 4 · 3 DJs', { tentative: true })
	];

	// SAFETY: the page only asks whether there are any.
	const act = { slug: 'neon-tide' } as ProjectView;

	const loading = new Promise<never>(() => {});

	const ago = (hours: number) => new Date(Date.now() - hours * 3_600_000).toISOString();

	const nothingPublished: LocallibraryStatus = { statsAt: null, sets: null };

	const statsOnly: LocallibraryStatus = { statsAt: ago(50), sets: null };

	const published: LocallibraryStatus = { statsAt: ago(50), sets: { href: '/@neon.test/sets/3kset', name: 'Tidal Hours at Halle Nord', at: ago(3) } };

	const base = {
		viewer: viewer(),
		pendingRequests: 0,
		unread: 0,
		sections: Promise.resolve(null),
		projects: Promise.resolve([act]),
		waiting: needs.length,
		needs,
		access: Promise.resolve(access),
		firstRun: null,
		summary: { others: '3 bookings are waiting on promoters and 1 request is with staff. Nothing needed from you.', hasBookings: true },
		nights: { nights, more: 0, failed: false },
		onboarding: null,
		locallibrary: published
	};

	const quiet = { ...base, waiting: 0, needs: [], access: Promise.resolve([]) };

	const fresh = { ...quiet, summary: { others: null, hasBookings: false }, projects: Promise.resolve([]), nights: { nights: [], more: 0, failed: false }, locallibrary: nothingPublished };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const newAccount = (hasAvatar: boolean): OnboardingData => ({ profile: null, prefill: { name: 'Pat', hasAvatar }, listing: false, follows: false });

	const partly: OnboardingData = {
		profile: { basics: { name: 'Pat', bio: 'Warehouse techno, Berlin.' }, performer: {}, createdAt: '', updatedAt: '' },
		prefill: null,
		listing: false,
		follows: true
	};
</script>

<Story name="Busy" asChild>
	<Page params={{}} form={null} data={base} />
</Story>

<Story name="Busy on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={base} />
</Story>

<Story name="More than a page holds" asChild>
	<Page params={{}} form={null} data={{ ...base, waiting: 6, needs: [...needs, ...needs.map((row) => ({ ...row, key: `${row.key}-again` }))], nights: { nights, more: 4, failed: false } }} />
</Story>

<!-- Nothing waits on them, so Needs you is gone and the line still says what's next. -->
<Story name="Quiet" asChild>
	<Page params={{}} form={null} data={quiet} />
</Story>

<Story name="Quiet on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={quiet} />
</Story>

<Story name="Nothing here yet" asChild>
	<Page params={{}} form={null} data={fresh} />
</Story>

<Story name="First visit" asChild>
	<Page params={{}} form={null} data={{ ...fresh, firstRun: { name: 'Pat', hasAvatar: true }, onboarding: newAccount(true) }} />
</Story>

<Story name="First visit on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={{ ...fresh, firstRun: { name: 'Pat', hasAvatar: true }, onboarding: newAccount(true) }} />
</Story>

<Story name="New account with a booking waiting" asChild>
	<Page params={{}} form={null} data={{ ...base, firstRun: { name: 'Pat', hasAvatar: false }, onboarding: newAccount(false) }} />
</Story>

<!-- The checklist stays first until it's done, ticking off what the profile has. -->
<Story name="Partly set up" asChild>
	<Page params={{}} form={null} data={{ ...quiet, onboarding: partly }} />
</Story>

<Story name="Partly set up on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={{ ...quiet, onboarding: partly }} />
</Story>

<Story name="Nights loading" asChild>
	<Page params={{}} form={null} data={{ ...base, nights: loading }} />
</Story>

<Story name="Loading" asChild>
	<Page params={{}} form={null} data={{ ...base, needs: loading, access: loading, projects: loading, firstRun: loading, summary: loading, nights: loading, onboarding: loading, locallibrary: loading }} />
</Story>

<Story name="Loading on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={{ ...base, needs: loading, access: loading, projects: loading, firstRun: loading, summary: loading, nights: loading, onboarding: loading, locallibrary: loading }} />
</Story>

<Story name="Some nights didn't load" asChild>
	<Page params={{}} form={null} data={{ ...base, nights: { nights: nights.slice(1), more: 0, failed: true } }} />
</Story>

<Story name="No nights loaded" asChild>
	<Page params={{}} form={null} data={{ ...base, nights: { nights: [], more: 0, failed: true } }} />
</Story>

<Story name="Server without private data" asChild>
	<Page params={{}} form={null} data={{ ...base, viewer: viewer(false) }} />
</Story>

<!-- The locallibrary card, under the checklist and everything that waits: nothing published offers locallibrary. -->
<Story name="Nothing published from locallibrary" asChild>
	<Page params={{}} form={null} data={{ ...quiet, onboarding: partly, locallibrary: nothingPublished }} />
</Story>

<Story name="Nothing published from locallibrary on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={{ ...quiet, onboarding: partly, locallibrary: nothingPublished }} />
</Story>

<Story name="Stats published, no sets" asChild>
	<Page params={{}} form={null} data={{ ...quiet, locallibrary: statsOnly }} />
</Story>

<Story name="Stats published, no sets, on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={{ ...quiet, locallibrary: statsOnly }} />
</Story>

<Story name="Stats and sets published" asChild>
	<Page params={{}} form={null} data={{ ...quiet, locallibrary: published }} />
</Story>

<Story name="Stats and sets published on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={{ ...quiet, locallibrary: published }} />
</Story>
