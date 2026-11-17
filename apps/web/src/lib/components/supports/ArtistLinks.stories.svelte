<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import ArtistLinks from './ArtistLinks.svelte';
	import { charts } from './fixtures.ts';

	const { Story } = defineMeta({ title: 'Supports/Artist links', component: ArtistLinks, tags: ['autodocs'] });

	// Low Tide (Dub) is Ferrule, who has an MBID, with Ava Lind, who doesn't.
	const [nightBus, lowTide] = charts.tracks;

	/** An artist with an MBID links by it, one without by name. */
	const linksEach = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		const canvas = within(canvasElement);

		await expect(canvas.getByRole('link', { name: 'Ferrule' })).toHaveAttribute('href', '/artists/2a8e4f60-1c3b-4d7e-9a05-6b1f0c2d3e41');
		await expect(canvas.getByRole('link', { name: 'Ava Lind' })).toHaveAttribute('href', '/artists/name/Ava%20Lind');
	};
</script>

<Story name="One artist" args={{ artists: nightBus.artists }} />

<Story name="Several artists" args={{ artists: lowTide.artists }} play={linksEach} />
