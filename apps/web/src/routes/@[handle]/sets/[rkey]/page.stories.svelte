<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { expect, within } from 'storybook/test';
	import { djSetSchema } from '#lib/dj-schema.ts';
	import { PAGE_META } from '#lib/testing/page-meta.ts';
	import { DEMO_DJ_SETS } from '../../../../../scripts/seed-data.ts';
	import Layout from '../../../+layout.svelte';
	import Page from './+page.svelte';

	/** One DJ's set at its own page: where and when, the event it was part of, its player and tracklist, and the mixes that record it. */
	const { Story } = defineMeta({
		title: 'Patterns/Set page',
		parameters: { layout: 'fullscreen', sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/@neon.test/sets/3kset') } } } }
	});

	const DID = 'did:plc:neon';

	const set = (index: number) => ({ ...djSetSchema.parse(DEMO_DJ_SETS[index]), uri: `at://${DID}/computer.localnetwork.dj.set/3kset${index}` });

	const loading = new Promise<never>(() => {});

	/** A club set with no event or mix, everything already in. */
	const club = {
		viewer: null,
		pendingRequests: 0,
		meta: PAGE_META,
		account: { did: DID, handle: 'neon.test' },
		name: 'Neon',
		avatarUrl: null,
		set: set(0),
		event: null,
		mixes: Promise.resolve([])
	};

	/** A studio mix on Neon Tide's page, with its tracklist report, and the event it premiered at. */
	const mix = {
		...club,
		set: { ...set(2), event: `at://${DID}/community.lexicon.calendar.event/3kevent` },
		event: Promise.resolve('Tidal Hours launch'),
		mixes: Promise.resolve([
			{ title: 'Tidal Hours 004', url: 'https://soundcloud.com/example-neon-tide/tidal-hours-004', project: { slug: 'neon-tide', name: 'Neon Tide' }, report: 1 },
			{ title: 'Tidal Hours 004 (radio edit)', url: 'https://soundcloud.com/example-neon-tide/tidal-hours-004-radio', project: { slug: 'saffron-social', name: 'Saffron Social' }, report: null }
		])
	};

	/** The event's name and the mixes still on their way. */
	const waiting = { ...mix, event: loading, mixes: loading };

	/** A set with nothing but its day: no title, place or tracks. The directory didn't answer about mixes. */
	const bare = { ...club, set: { ...djSetSchema.parse({ date: '2026-10-02', tracks: [] }), uri: `at://${DID}/computer.localnetwork.dj.set/3kbare` }, mixes: Promise.resolve(null) };

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

{#snippet page(data: typeof club | typeof mix | typeof waiting | typeof bare)}
	<Layout data={{ viewer: null, pendingRequests: 0 }} params={{}}>
		<Page {data} params={{ handle: 'neon.test', rkey: '3kset' }} form={null} />
	</Layout>
{/snippet}

<Story
	name="Club set"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole('heading', { level: 1, name: 'Tidal Hours' })).toBeVisible();
		await expect(canvas.getByRole('link', { name: /Neon$/ })).toHaveAttribute('href', '/@neon.test');
		await expect(canvas.queryByRole('heading', { name: 'Shared as a mix' })).toBeNull();
	}}
>
	{@render page(club)}
</Story>

<Story name="Club set on a phone" asChild globals={phone}>{@render page(club)}</Story>

<Story
	name="Shared as a mix"
	asChild
	play={async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(await canvas.findByText(/Part of Tidal Hours launch$/)).toBeVisible();
		await expect(await canvas.findByRole('link', { name: 'Tidal Hours 004' })).toHaveAttribute('href', '/projects/neon-tide/mixes/1');
		await expect(canvas.getByRole('button', { name: 'Play Tidal Hours 004' })).toBeVisible();
	}}
>
	{@render page(mix)}
</Story>

<Story name="Shared as a mix on a phone" asChild globals={phone}>{@render page(mix)}</Story>

<Story
	name="Event and mixes loading"
	asChild
	play={async ({ canvasElement }) => {
		await expect(within(canvasElement).getByText('Loading the event')).toBeInTheDocument();
	}}
>
	{@render page(waiting)}
</Story>

<Story name="Event and mixes loading on a phone" asChild globals={phone}>{@render page(waiting)}</Story>

<!-- Nothing to list, and the directory couldn't say which mixes record it: the page shows the set alone. -->
<Story
	name="Bare set, directory offline"
	asChild
	play={async ({ canvasElement }) => {
		await expect(within(canvasElement).getByRole('heading', { level: 1, name: 'Untitled set' })).toBeVisible();
	}}
>
	{@render page(bare)}
</Story>

<Story name="Bare set, directory offline, on a phone" asChild globals={phone}>{@render page(bare)}</Story>
