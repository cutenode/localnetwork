<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import { neonPlaying } from '#lib/testing/profiles.ts';
	import { desktop, phone } from '#lib/testing/viewports.ts';
	import DjSets from './DjSets.svelte';

	const [{ sets, handle }] = neonPlaying();

	/** Neon's sets from the seed data, newest first. The last has a SoundCloud link, so it plays here. */
	const { Story } = defineMeta({ title: 'Mixes/DJ sets', component: DjSets, tags: ['autodocs'], args: { sets, handle } });
</script>

<Story name="Sets" globals={desktop} />

<!-- On the set's own page, where its name doesn't link. -->
<Story name="On its page" args={{ sets: sets.slice(0, 1), link: false }} globals={desktop} />

<Story name="Phone" globals={phone} />

<!-- A set's tracks, each artist linking to who else played them. -->
<Story
	name="Tracklist open"
	globals={desktop}
	play={async ({ canvasElement }) => {
		await userEvent.click(within(canvasElement).getAllByText('Tracklist')[0]);
		await expect(canvasElement.querySelector('details')).toHaveAttribute('open');
	}}
/>
