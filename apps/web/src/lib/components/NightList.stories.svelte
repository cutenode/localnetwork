<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { NightRow } from '#lib/home.ts';
	import NightList from './NightList.svelte';

	const { Story } = defineMeta({ title: 'Dashboard/Night list', component: NightList, tags: ['autodocs'] });

	const night = (key: string, date: string, title: string, detail: string, overrides: Partial<NightRow> = {}): NightRow => ({ key, date, title, detail, href: '/dashboard/planner', tentative: false, ...overrides });

	const nights = [
		night('a', '2026-11-06', 'Friday Social', 'You play 01:00 as Neon Tide · Tiefgarage', { when: 'Tonight' }),
		night('b', '2026-11-07', 'Late Room', 'You play 23:00 as Saffron Social', { when: 'Tomorrow' }),
		night('c', '2026-11-14', 'Rooftop Rave, a long show name that runs past the edge of a phone', 'Booking for Neon Tide · not confirmed', { tentative: true }),
		night('d', '2026-12-12', 'Winter Social', 'Planned · Halle 4 · 3 DJs', { tentative: true })
	];

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Settled and not" args={{ nights }} />

<Story name="All settled" args={{ nights: nights.map((listed) => ({ ...listed, tentative: false })) }} />

<Story name="Loading" args={{ loading: true }} />

<Story name="Phone" args={{ nights }} globals={phone} />
