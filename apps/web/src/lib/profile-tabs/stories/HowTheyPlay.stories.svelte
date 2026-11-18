<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { phone } from '#lib/testing/viewports.ts';
	import HowTheyPlay from '../HowTheyPlay.svelte';
	import { dj, neon } from './fixtures.ts';

	/** The How they play tab's panel on its own; Patterns/Profile tabs draws it among the others. */
	const { Story } = defineMeta({ title: 'Profile tabs/How they play', component: HowTheyPlay, tags: ['autodocs'] });

	const data = { profile: neon, sets: undefined, dj, playsIn: [] };
</script>

<Story name="Stats" args={{ data }} />

<Story name="Phone" args={{ data }} globals={phone} />
