<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { ProjectProfile } from '#lib/types.ts';
	import PerformerFacts from './PerformerFacts.svelte';

	const { Story } = defineMeta({ title: 'Patterns/Performer facts', component: PerformerFacts, tags: ['autodocs'] });

	const profile: ProjectProfile = {
		name: 'Neon Tide',
		kind: 'solo',
		genres: ['techno'],
		location: { city: 'Berlin', country: 'DE' },
		setTypes: ['peak', 'closing'],
		travel: { radiusKm: 600, international: true }
	};

	const fees: ProjectProfile['fees'] = [
		{ label: 'Club nights', tier: 'established', currency: 'EUR', minAmount: 600, maxAmount: 1500 },
		{ label: 'Festivals', tier: 'headliner', currency: 'EUR', minAmount: 2000 },
		{ tier: 'community' }
	];

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Fee levels" args={{ performer: { ...profile, fees } }} />

<Story name="One fee" args={{ performer: { ...profile, fees: [{ tier: 'emerging' }] } }} />

<Story name="No fees" args={{ performer: profile }} />

<Story name="A DJ's performer section" args={{ performer: { setTypes: ['closing'], activeSince: 2016 } }} />

<Story name="Phone, fee levels" args={{ performer: { ...profile, fees } }} globals={phone} />
