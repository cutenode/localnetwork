<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import { inquiryDefaults } from '#lib/shows.ts';
	import type { Viewer } from '#lib/types.ts';
	import Page from './+page.svelte';

	/** The booking inquiry form, to an act's managers or to a DJ Find DJs lists, who answers for themself. */
	const { Story } = defineMeta({
		title: 'Patterns/Booking inquiry',
		parameters: { sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/dashboard/bookings/new?dj=did:plc:neon') } } } }
	});

	const viewer: Viewer = { did: 'did:plc:pat', handle: 'pat.test', roles: [], isAdmin: false, isStaff: false, spacesCapable: true };

	/** Neon, listed in Find DJs, asking for a fee in euros. */
	const dj = {
		viewer,
		pendingRequests: 0,
		unread: 0,
		waiting: 0,
		needs: [],
		access: Promise.resolve([]),
		sections: Promise.resolve(null),
		projects: Promise.resolve([]),
		to: { uri: 'at://did:plc:neon/computer.localnetwork.profile/self', name: 'Neon', avatarUrl: null, place: 'Berlin, DE', href: '/@neon.test', dj: true },
		values: { ...inquiryDefaults, currency: 'EUR' },
		show: null,
		asks: ['Fee: Established · €600–€1,500.'],
		geocoder: false
	};

	/** The act Neon Tide, whose managers answer. */
	const act = { ...dj, to: { uri: 'at://did:plc:root/computer.localnetwork.project/2', name: 'Neon Tide', avatarUrl: null, place: 'Berlin, DE', href: '/projects/neon-tide', dj: false }, asks: [] };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story
	name="To a listed DJ"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText(/private thread with Neon\./)).toBeVisible();
		await expect(canvas.getByRole('link', { name: 'Cancel' })).toHaveAttribute('href', '/@neon.test');
	}}
>
	<Page data={dj} params={{}} form={null} />
</Story>

<Story name="To a listed DJ on a phone" asChild globals={phone}><Page data={dj} params={{}} form={null} /></Story>

<Story
	name="To an act"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText(/private thread with the project's managers/)).toBeVisible();
	}}
>
	<Page data={act} params={{}} form={null} />
</Story>
