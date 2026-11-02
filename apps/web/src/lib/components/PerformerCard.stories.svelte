<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { PerformerSummary } from '#lib/performers.ts';
	import PerformerCard from './PerformerCard.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Performer card', component: PerformerCard, tags: ['autodocs'] });

	const project: PerformerSummary = {
		href: '/projects/saffron-social',
		name: 'Saffron Social',
		avatarUrl: null,
		kind: 'Duo',
		place: 'Berlin, DE',
		distanceKm: 3.2,
		unclaimed: false,
		tagline: 'Disco, boogie, and edits for daytime dancers',
		genres: ['disco', 'boogie', 'edits', 'nu-disco'],
		fee: 'Emerging'
	};

	const dj: PerformerSummary = {
		href: '/@neon.test',
		name: 'Neon',
		avatarUrl: null,
		kind: 'Solo DJ',
		place: null,
		distanceKm: null,
		unclaimed: false,
		genres: ['Techno', 'Ambient'],
		fee: ''
	};

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="A project" args={{ performer: project }} />

<Story name="A listed DJ with only what they filled in" args={{ performer: dj }} />

<Story name="Unclaimed project" args={{ performer: { ...project, unclaimed: true, distanceKm: null } }} />

<Story name="Phone, a project" args={{ performer: project }} globals={phone} />
