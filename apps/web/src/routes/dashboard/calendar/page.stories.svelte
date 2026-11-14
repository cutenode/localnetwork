<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { NightRow } from '#lib/home.ts';
	import type { ProjectView, Viewer } from '#lib/types.ts';
	import Page from './+page.svelte';

	/** Calendar: every night the viewer is part of for a year, by month, with unsettled ones dashed. */
	const { Story } = defineMeta({
		title: 'Patterns/Dashboard calendar',
		parameters: { sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/dashboard/calendar') } } } }
	});

	const viewer: Viewer = { did: 'did:plc:neon', handle: 'neon.test', roles: [], isAdmin: false, isStaff: false, spacesCapable: true };

	const night = (key: string, date: string, title: string, detail: string, overrides: Partial<NightRow> = {}): NightRow => ({ key, date, title, detail, href: '/dashboard/bookings/3k1', tentative: false, ...overrides });

	const nights = [
		night('a', '2026-11-07', 'Friday Social', 'You play 01:00 as Neon Tide · Tiefgarage', { when: 'Tonight' }),
		night('b', '2026-11-14', 'Rooftop Rave', 'Booking for Neon Tide · not confirmed', { tentative: true, inquiries: 2 }),
		night('c', '2026-11-14', 'Basement Late', 'Booking for Neon Tide · not confirmed', { tentative: true }),
		night('d', '2026-11-21', 'Closing Night', 'Booking for Neon Tide'),
		night('e', '2026-12-12', 'Winter Social', 'Planned · Halle 4 · 3 DJs', { tentative: true }),
		night('f', '2027-01-09', 'New Year Warehouse', 'You play 23:00 as Second Act · Halle 4')
	];

	// SAFETY: the page reads only an act's slug and status.
	const act = { slug: 'neon-tide', status: 'active' } as ProjectView;

	const loading = new Promise<never>(() => {});

	const base = {
		viewer,
		pendingRequests: 0,
		unread: 0,
		sections: Promise.resolve({ performer: true, promoter: true }),
		projects: Promise.resolve([act]),
		waiting: 0,
		needs: [],
		access: Promise.resolve([]),
		nights: { nights, more: 0, failed: false }
	};

	const empty = { nights: [], more: 0, failed: false };

	const promoter = { ...base, sections: Promise.resolve({ performer: false, promoter: true }), projects: Promise.resolve([]), nights: empty };

	const dj = { ...base, sections: Promise.resolve({ performer: true, promoter: false }), nights: empty };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Nights ahead" asChild>
	<Page params={{}} form={null} data={base} />
</Story>

<Story name="Nights ahead on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={base} />
</Story>

<Story name="Loading" asChild>
	<Page params={{}} form={null} data={{ ...base, nights: loading }} />
</Story>

<Story name="Loading on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={{ ...base, nights: loading }} />
</Story>

<Story name="Nothing coming up, promoter" asChild>
	<Page params={{}} form={null} data={promoter} />
</Story>

<Story name="Nothing coming up, DJ" asChild>
	<Page params={{}} form={null} data={dj} />
</Story>

<Story name="Nothing coming up on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={promoter} />
</Story>

<Story name="Some nights didn't load" asChild>
	<Page params={{}} form={null} data={{ ...base, nights: { nights: nights.slice(1), more: 0, failed: true } }} />
</Story>

<Story name="Some nights didn't load on a phone" asChild globals={phone}>
	<Page params={{}} form={null} data={{ ...base, nights: { nights: nights.slice(1), more: 0, failed: true } }} />
</Story>

<Story name="No nights loaded" asChild>
	<Page params={{}} form={null} data={{ ...base, nights: { ...empty, failed: true } }} />
</Story>
