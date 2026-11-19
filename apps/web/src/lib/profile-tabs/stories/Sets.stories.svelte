<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { phone } from '#lib/testing/viewports.ts';
	import Sets from '../Sets.svelte';
	import { dj, neon } from './fixtures.ts';

	/** The Sets tab's panel on its own; Patterns/Profile tabs draws it among the others. */
	const { Story } = defineMeta({ title: 'Profile tabs/Sets', component: Sets, tags: ['autodocs'] });

	const data = { profile: neon, sets: undefined, dj, playsIn: [] };
</script>

<Story name="Sets" args={{ data }} />

<Story name="Phone" args={{ data }} globals={phone} />
