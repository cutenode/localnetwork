<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, userEvent, within } from 'storybook/test';
	import { NEON_CREW, neonPlaying, neonProfile } from '#lib/testing/profiles.ts';
	import type { ProfileView } from '#lib/types.ts';
	import ProfileTabs from './ProfileTabs.svelte';

	const profile = (performer: NonNullable<ProfileView['record']['performer']>, bio?: string) =>
		neonProfile({ basics: { name: 'Neon', bio }, performer });

	const neon = profile(
		{
			mixes: [{ title: 'Warehouse Hours', url: 'https://soundcloud.com/neon/warehouse' }],
			highlights: ['Four-year residency at Halle Nord']
		},
		'Plays long techno sets in Berlin, and runs a monthly warehouse night with friends.'
	);

	const dj = neonPlaying({ stats: true });

	const playsIn = [NEON_CREW];

	const waiting = new Promise<never>(() => {});

	/** A person's profile sections as tabs, with Neon from the seed data; the first two at 1280px, the rest at 390px too. */
	const { Story } = defineMeta({
		title: 'Patterns/Profile tabs',
		component: ProfileTabs,
		tags: ['autodocs'],
		args: { profile: neon, sets: undefined, dj, playsIn, requested: null },
		parameters: {
			viewport: { options: { phone: { name: 'Phone', styles: { width: '390px', height: '844px' }, type: 'mobile' } } }
		}
	});

	const desktop = { viewport: { value: 'desktop', isRotated: false } };

	const phone = { viewport: { value: 'phone', isRotated: false } };
</script>

<Story
	name="Every tab"
	globals={desktop}
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const tab = (name: string) => canvas.getByRole('tab', { name });

		await expect(canvas.getAllByRole('tab').map((link) => link.textContent)).toEqual(['About', 'Mixes', 'Sets', 'How they play', 'Plays in']);
		await expect(tab('About')).toHaveAttribute('aria-selected', 'true');
		tab('About').focus();
		await userEvent.keyboard('{ArrowRight}');
		await expect(tab('Mixes')).toHaveFocus();
		await userEvent.keyboard('{End}');
		await expect(tab('Plays in')).toHaveFocus();
	}}
/>

<Story name="Every tab on a phone" globals={phone} />

<Story
	name="Linked to Sets"
	args={{ requested: 'sets' }}
	globals={desktop}
	play={async ({ canvasElement }) => {
		await expect(within(canvasElement).getByRole('tabpanel', { name: 'Sets' })).toBeInTheDocument();
	}}
/>

<Story name="Linked to Sets on a phone" args={{ requested: 'sets' }} globals={phone} />

<!-- One section shows under its heading, with no tabs to choose between. -->
<Story
	name="One section"
	args={{ profile: profile({}, 'Plays long techno sets in Berlin.'), dj: [], playsIn: [] }}
	globals={phone}
	play={async ({ canvasElement }) => {
		await expect(within(canvasElement).queryByRole('tablist')).toBeNull();
	}}
/>

<!-- Sets, stats and projects still on their way, with nothing else to show yet. -->
<Story name="Loading" args={{ profile: profile({}), dj: waiting, playsIn: waiting }} globals={phone} />

<Story name="Nothing yet" args={{ profile: profile({}), dj: [], playsIn: [] }} globals={phone} />

<!-- The directory is offline, so the projects they play in can't be listed: that tab is left out. -->
<Story
	name="Directory offline"
	args={{ playsIn: null }}
	globals={phone}
	play={async ({ canvasElement }) => {
		await expect(within(canvasElement).queryByRole('tab', { name: 'Plays in' })).toBeNull();
	}}
/>
