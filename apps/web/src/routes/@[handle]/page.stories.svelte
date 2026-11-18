<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import { NEON, NEON_CREW, neonProfile } from '#lib/testing/profiles.ts';
	import { PAGE_META } from '#lib/testing/page-meta.ts';
	import type { FollowState, Viewer } from '#lib/types.ts';
	import { phone } from '#lib/testing/viewports.ts';
	import Layout from '../+layout.svelte';
	import Page from './+page.svelte';

	/** A person's profile, with its Follow button in each state a visitor can find it in, and Send inquiry on a listed DJ's. */
	const { Story } = defineMeta({
		title: 'Patterns/Profile page',
		parameters: { layout: 'fullscreen', sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/@neon.test') } } } }
	});

	const fan: Viewer = { did: 'did:plc:fan', handle: 'fan.test', roles: [], isAdmin: false, isStaff: false, spacesCapable: true };

	const owner: Viewer = { ...fan, did: NEON, handle: 'neon.test' };

	const profile = neonProfile({
		basics: { name: 'Neon Tide', location: { city: 'Berlin', country: 'DE' }, pronouns: 'they/them' },
		performer: { tagline: 'Four hours of rolling techno', genres: ['techno', 'electro'] }
	});

	/**
	 * Neon's page as `viewer` sees it, with `follow` streaming in (undefined when there's no Follow), and `bookable` when
	 * staff list them in Find DJs and they take inquiries. `saved` is whether they're in the viewer's planner, null when
	 * the viewer can't save them.
	 */
	const data = (viewer: Viewer | null, follow?: Promise<FollowState | null>, bookable = false, saved: boolean | null = null) => ({
		viewer,
		pendingRequests: 0,
		profile,
		name: 'Neon Tide',
		pronouns: 'they/them',
		account: { did: NEON, handle: 'neon.test' },
		bookable,
		meta: PAGE_META,
		dj: Promise.resolve([]),
		sets: undefined,
		playsIn: Promise.resolve([NEON_CREW]),
		follow,
		saved
	});

	const followState = (followers: number, following = false) => Promise.resolve({ followers, following });
</script>

{#snippet page(pageData: ReturnType<typeof data>, form: { error: string; values: undefined; reauth: boolean } | null = null)}
	<Layout data={{ viewer: pageData.viewer, pendingRequests: 0 }} params={{}}>
		<Page data={pageData} params={{ handle: 'neon.test' }} {form} />
	</Layout>
{/snippet}

<Story
	name="Signed out"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(await canvas.findByRole('link', { name: 'Follow 1,204 followers' })).toHaveAttribute('href', '/login?next=%2F%40neon.test%23follow');
	}}
>
	{@render page(data(null, followState(1204)))}
</Story>

<Story name="Signed out on a phone" asChild globals={phone}>{@render page(data(null, followState(1204)))}</Story>

<Story
	name="Following"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(await canvas.findByRole('button', { name: 'Following 13 followers' })).toHaveAttribute('aria-pressed', 'true');
	}}
>
	{@render page(data(fan, followState(13, true)))}
</Story>

<Story name="Following on a phone" asChild globals={phone}>{@render page(data(fan, followState(13, true)))}</Story>

<Story name="Followers loading" asChild>{@render page(data(fan, new Promise<never>(() => {})))}</Story>

<Story name="Directory offline" asChild>{@render page(data(fan, Promise.resolve(null)))}</Story>

<Story
	name="Listed DJ"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole('link', { name: 'Send inquiry' })).toHaveAttribute('href', `/dashboard/bookings/new?dj=${NEON}`);
	}}
>
	{@render page(data(fan, followState(13), true))}
</Story>

<Story name="Listed DJ on a phone" asChild globals={phone}>{@render page(data(fan, followState(13), true))}</Story>

<Story name="Listed DJ, saved to the planner" asChild>{@render page(data(fan, followState(13), true, true))}</Story>

<Story name="Listed DJ, saved, on a phone" asChild globals={phone}>{@render page(data(fan, followState(13), true, true))}</Story>

<Story
	name="Listed DJ, signed out"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole('link', { name: 'Sign in to book' })).toHaveAttribute('href', '/login?next=%2F%40neon.test');
	}}
>
	{@render page(data(null, followState(1204), true))}
</Story>

<Story
	name="Unlisted DJ"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.queryByRole('link', { name: 'Send inquiry' })).toBeNull();
	}}
>
	{@render page(data(fan, followState(13)))}
</Story>

<Story
	name="Your own profile"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole('link', { name: 'Manage' })).toBeVisible();
		await expect(canvas.queryByRole('button', { name: /^Follow/ })).toBeNull();
		await expect(canvas.queryByRole('link', { name: 'Send inquiry' })).toBeNull();
	}}
>
	{@render page(data(owner, undefined, true))}
</Story>

<Story name="Follow failed" asChild>{@render page(data(fan, followState(12)), { error: "Following isn't available right now. Try again in a minute.", values: undefined, reauth: false })}</Story>

<Story name="Follow failed on a phone" asChild globals={phone}>{@render page(data(fan, followState(12)), { error: "Following isn't available right now. Try again in a minute.", values: undefined, reauth: false })}</Story>

<Story name="Sign in again to follow" asChild>{@render page(data(fan, followState(12)), { error: 'Sign in again to follow DJs', values: undefined, reauth: true })}</Story>
