<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import { PAGE_META } from '#lib/testing/page-meta.ts';
	import { NEON, neonProfile } from '#lib/testing/profiles.ts';
	import type { Viewer } from '#lib/types.ts';
	import Layout from '../+layout.svelte';
	import Page from './+page.svelte';

	/** A DJ's profile, with what a visitor can do from it: send a listed DJ an inquiry, or sign in to. */
	const { Story } = defineMeta({
		title: 'Patterns/Profile page',
		parameters: { layout: 'fullscreen', sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/@neon.test') } } } }
	});

	const pat: Viewer = { did: 'did:plc:pat', handle: 'pat.test', roles: [], isAdmin: false, isStaff: false, spacesCapable: true };

	const profile = neonProfile({
		basics: { name: 'Neon', location: { city: 'Berlin', country: 'DE' } },
		performer: { tagline: 'Four hours of rolling techno', genres: ['techno', 'electro'], activeSince: 2014 }
	});

	/** A DJ Find DJs lists, seen by a promoter. */
	const listed = {
		viewer: pat,
		pendingRequests: 0,
		meta: PAGE_META,
		profile,
		name: 'Neon',
		account: { did: NEON, handle: 'neon.test' },
		bookable: true,
		dj: Promise.resolve([]),
		sets: undefined,
		playsIn: Promise.resolve([])
	};

	const signedOut = { ...listed, viewer: null };

	/** The DJ on their own page: Manage, never an inquiry to themself. */
	const own = { ...listed, viewer: { ...pat, did: NEON, handle: 'neon.test' } };

	/** A DJ staff haven't listed: no inquiry. */
	const unlisted = { ...listed, bookable: false };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

{#snippet page(data: typeof listed | typeof signedOut)}
	<Layout data={{ viewer: data.viewer, pendingRequests: 0 }} params={{}}>
		<Page {data} params={{ handle: 'neon.test' }} form={null} />
	</Layout>
{/snippet}

<Story
	name="Listed DJ"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole('link', { name: 'Send inquiry' })).toHaveAttribute('href', `/dashboard/bookings/new?dj=${NEON}`);
	}}
>
	{@render page(listed)}
</Story>

<Story name="Listed DJ on a phone" asChild globals={phone}>{@render page(listed)}</Story>

<Story
	name="Listed DJ, signed out"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole('link', { name: 'Sign in to book' })).toHaveAttribute('href', '/login?next=%2F%40neon.test');
	}}
>
	{@render page(signedOut)}
</Story>

<Story
	name="Their own page"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole('link', { name: 'Manage' })).toBeVisible();
		await expect(canvas.queryByRole('link', { name: 'Send inquiry' })).toBeNull();
	}}
>
	{@render page(own)}
</Story>

<Story
	name="Unlisted DJ"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.queryByRole('link', { name: 'Send inquiry' })).toBeNull();
	}}
>
	{@render page(unlisted)}
</Story>
