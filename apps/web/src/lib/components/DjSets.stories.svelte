<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { neonPlaying } from '#lib/testing/profiles.ts';
	import DjSets from './DjSets.svelte';

	const [{ sets, handle }] = neonPlaying();

	/** Neon's sets from the seed data, newest first. The last has a SoundCloud link, so it plays here. */
	const { Story } = defineMeta({ title: 'Mixes/DJ sets', component: DjSets, tags: ['autodocs'], args: { sets, handle } });

	const desktop = { viewport: { value: 'desktop', isRotated: false } };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Sets" globals={desktop} />

<!-- On the set's own page, where its name doesn't link. -->
<Story name="On its page" args={{ sets: sets.slice(0, 1), link: false }} globals={desktop} />

<Story name="Phone" globals={phone} />
