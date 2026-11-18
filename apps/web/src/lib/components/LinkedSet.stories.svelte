<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { artistSupports } from './supports/fixtures.ts';
	import LinkedSet from './LinkedSet.svelte';

	// Neon's Halle Nord set, as the directory has it.
	const { set } = artistSupports.tracks[0].plays[0];

	const linked = { uri: set.uri, cid: set.cid };

	/** The set a mix was recorded at, under the mix. */
	const { Story } = defineMeta({
		title: 'Mixes/Linked set',
		component: LinkedSet,
		tags: ['autodocs'],
		args: { linked, sets: Promise.resolve(new Map([[set.uri, set]])) }
	});
</script>

<Story name="Linked set" />

<!-- The set was edited after the mix linked it. -->
<Story name="Changed since linked" args={{ linked: { ...linked, cid: 'bafyolder' } }} />

<Story name="Loading" args={{ sets: new Promise<never>(() => {}) }} />

<Story name="No longer published" args={{ sets: Promise.resolve(new Map()) }} />

<!-- The directory couldn't say, so it links the set's page. -->
<Story name="Directory offline" args={{ sets: Promise.resolve(null) }} />
