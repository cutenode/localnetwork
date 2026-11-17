<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import PlaysIn from '../PlaysIn.svelte';
	import { neon, playsIn } from './fixtures.ts';

	/** The Plays in tab's panel on its own; Patterns/Profile tabs draws it among the others. */
	const { Story } = defineMeta({ title: 'Profile tabs/Plays in', component: PlaysIn, tags: ['autodocs'] });
</script>

<Story name="Projects" args={{ data: { profile: neon, sets: undefined, dj: [], playsIn } }} />
