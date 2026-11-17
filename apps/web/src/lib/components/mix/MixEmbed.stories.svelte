<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import { embedUrl } from '#lib/mix/links.ts';
	import { light } from '#lib/testing/light.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import MixEmbed from './MixEmbed.svelte';

	const { Story } = defineMeta({ title: 'Mixes/Platform player', component: MixEmbed, tags: ['autodocs'] });

	/** The example tracks from SoundCloud's and Mixcloud's widget docs. The story tests answer them with a stub (.storybook/stub-players.ts). */
	const soundcloud = { id: 'player-soundcloud', title: 'Flickermood', src: embedUrl('https://soundcloud.com/forss/flickermood') ?? '' };

	const mixcloud = { id: 'player-mixcloud', title: 'Party Time', src: embedUrl('https://www.mixcloud.com/spartacus/party-time/') ?? '' };

	/** The player gets the height its platform lays out for, and Mixcloud's is asked for the theme's palette. */
	const sized =
		(title: string, height: number, palette: string | null = null) =>
		async ({ canvasElement }: { canvasElement: HTMLElement }) => {
			const player = within(canvasElement).getByTitle(title);

			await expect(player.getBoundingClientRect().height).toBe(height);
			await expect(new URL(player.getAttribute('src') ?? '').searchParams.get('light')).toBe(palette);
		};
</script>

<Story name="SoundCloud" args={soundcloud} play={sized('Flickermood', 166)} />

<Story name="Mixcloud" args={mixcloud} play={sized('Party Time', 120)} />

<Story name="Mixcloud, light" args={mixcloud} play={sized('Party Time', 120, '1')} beforeEach={light} />

<Story name="Phone" args={soundcloud} globals={phone} />
