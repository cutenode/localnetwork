<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import { phone } from '#lib/testing/viewports.ts';
	import ArtistPage from './ArtistPage.svelte';
	import { artistSupports } from './fixtures.ts';

	/** An artist's page: the way back to Artists over their supports. Retry asks for the page it's on. */
	const { Story } = defineMeta({
		title: 'Supports/Artist page',
		component: ArtistPage,
		tags: ['autodocs'],
		parameters: { layout: 'fullscreen', sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/artists/name/Ferrule?from=charts') } } } }
	});

	const backToArtists = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		await expect(within(canvasElement).getByRole('link', { name: 'Artists' })).toHaveAttribute('href', '/artists');
	};

	const retriesHere = async ({ canvasElement }: { canvasElement: HTMLElement }) => {
		await expect(within(canvasElement).getByRole('link', { name: 'Try again' })).toHaveAttribute('href', '/artists/name/Ferrule?from=charts');
	};
</script>

<Story name="Plays" args={{ supports: artistSupports }} play={backToArtists} />

<Story name="No plays" args={{ supports: { tracks: [], djs: {} } }} />

<Story name="Offline" args={{ supports: null }} play={retriesHere} />

<Story name="Phone" args={{ supports: artistSupports }} globals={phone} />
