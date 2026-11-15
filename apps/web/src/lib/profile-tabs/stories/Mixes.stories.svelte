<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { artistSupports } from '#lib/components/supports/fixtures.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import Mixes from '../Mixes.svelte';
	import { neon, profile, waiting } from './fixtures.ts';

	/** The Mixes tab's panel on its own; Patterns/Profile tabs draws it among the others. */
	const { Story } = defineMeta({ title: 'Profile tabs/Mixes', component: Mixes, tags: ['autodocs'] });

	// Neon's Halle Nord set, which the second mix was recorded at.
	const { set } = artistSupports.tracks[0].plays[0];

	const linked = profile({
		mixes: [
			...(neon.record.performer?.mixes ?? []),
			{ title: 'Halle Nord closing set', url: 'https://soundcloud.com/neon/halle-nord', set: { uri: set.uri, cid: set.cid } }
		]
	});

	const data = { profile: neon, sets: undefined, dj: [], playsIn: [] };
</script>

<Story name="Mixes" args={{ data }} />

<Story name="Linked to a set" args={{ data: { ...data, profile: linked, sets: Promise.resolve(new Map([[set.uri, set]])) } }} />

<!-- The directory is still looking the set up. -->
<Story name="Loading the set" args={{ data: { ...data, profile: linked, sets: waiting } }} />

<Story name="Phone" args={{ data: { ...data, profile: linked, sets: Promise.resolve(new Map([[set.uri, set]])) } }} globals={phone} />
