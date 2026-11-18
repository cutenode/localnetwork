<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { PAGE_META } from '#lib/testing/page-meta.ts';
	import { phone, tablet } from '#lib/testing/viewports.ts';
	import Layout from '../+layout.svelte';
	import ForLayout from './+layout.svelte';
	import type { PageData } from './$types';
	import Page from './+page.svelte';

	/** The page that sends promoters and DJs each to their own landing page, in the real layout. */
	const { Story } = defineMeta({
		title: 'Patterns/For',
		parameters: { layout: 'fullscreen', sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/for'), route: { id: '/for' } } } } }
	});

	const live: PageData['stats'] = Promise.resolve({ djs: 12, cities: 10, unclaimed: 7 });
</script>

{#snippet landing(stats: PageData['stats'])}
	{@const layout = { viewer: null, pendingRequests: 0, meta: PAGE_META }}
	<Layout data={layout} params={{}}>
		<ForLayout data={layout} params={{}}><Page data={{ ...layout, stats }} params={{}} form={null} /></ForLayout>
	</Layout>
{/snippet}

<Story name="Full page" asChild>{@render landing(live)}</Story>

<Story name="Full page on a tablet" asChild globals={tablet}>{@render landing(live)}</Story>

<Story name="Full page on a phone" asChild globals={phone}>{@render landing(live)}</Story>

<!-- The doors paint at once; the totals line holds its height until the directory answers. -->
<Story name="Loading" asChild>{@render landing(new Promise(() => {}))}</Story>

<Story name="Directory offline" asChild>{@render landing(Promise.resolve(null))}</Story>

<Story name="Directory offline on a phone" asChild globals={phone}>{@render landing(Promise.resolve(null))}</Story>
