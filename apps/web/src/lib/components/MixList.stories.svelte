<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import MixList from './MixList.svelte';

	/** The example tracks from SoundCloud's and Mixcloud's widget docs, so the open players show a real mix in Storybook. */
	const mixes = [
		{ title: 'Flickermood', url: 'https://soundcloud.com/forss/flickermood', durationMinutes: 4, year: 2007 },
		{ title: 'Party Time', url: 'https://www.mixcloud.com/spartacus/party-time/', durationMinutes: 56, year: 2012 },
		{ title: 'Tidal Hours', url: 'https://example.com/tidal-hours.mp3', durationMinutes: 92 }
	];

	const { Story } = defineMeta({ title: 'Mixes/Mix list', component: MixList, tags: ['autodocs'], args: { mixes, tracklistHref: () => '#tracklist' } });

	/** The light palette (packages/tokens/theme.css), which applies under a `light` class on the root. */
	const light = () => {
		document.documentElement.classList.add('light');

		return () => document.documentElement.classList.remove('light');
	};

	/**
	 * Opens a mix's player, as pressing its Play button does, and checks the player was asked for the theme's options
	 * at its height. The story tests answer the players with a stub (.storybook/stub-players.ts).
	 */
	const open =
		(title: string, height: number, options: Record<string, string | null>) =>
		async ({ canvasElement }: { canvasElement: HTMLElement }) => {
			const canvas = within(canvasElement);

			await userEvent.click(canvas.getByRole('button', { name: `Play ${title}` }));

			const player = canvas.getByTitle(title);
			const asked = new URL(player.getAttribute('src') ?? '').searchParams;

			await expect(player).toBeVisible();
			await expect(player.getBoundingClientRect().height).toBe(height);
			await expect(Object.fromEntries(Object.keys(options).map((name) => [name, asked.get(name)]))).toEqual(options);
		};
</script>

<Story name="Mixes" />

<Story name="SoundCloud player open" play={open('Flickermood', 166, { inverse: null })} />

<Story name="SoundCloud player open, light" play={open('Flickermood', 166, { inverse: null })} beforeEach={light} />

<Story name="Mixcloud player open" play={open('Party Time', 120, { light: null })} />

<Story name="Mixcloud player open, light" play={open('Party Time', 120, { light: '1' })} beforeEach={light} />
