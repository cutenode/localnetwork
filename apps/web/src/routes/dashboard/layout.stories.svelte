<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import RootLayout from '../+layout.svelte';
	import Layout from './+layout.svelte';
	import { promoterPage } from './promoter/fixtures.ts';
	import Page from './promoter/+page.svelte';

	/** The dashboard's sidebar by the sections a profile has on: a Promoter section only for someone promoting. */
	const { Story } = defineMeta({
		title: 'Patterns/Dashboard sidebar',
		parameters: { layout: 'fullscreen', sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/dashboard/promoter') } } } }
	});

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const promoting = promoterPage();

	const djOnly = promoterPage({ sections: Promise.resolve({ performer: true, promoter: false }) });

	const noProfile = promoterPage({ sections: Promise.resolve(null) });
</script>

{#snippet dashboard(data: typeof promoting)}
	<RootLayout data={{ viewer: data.viewer, pendingRequests: 0 }} params={{}}>
		<Layout {data} params={{}}><Page {data} params={{}} form={null} /></Layout>
	</RootLayout>
{/snippet}

<Story name="Promoting" asChild>{@render dashboard(promoting)}</Story>

<Story name="Promoting on a phone" asChild globals={phone}>{@render dashboard(promoting)}</Story>

<!-- A DJ who doesn't promote: no Promoter section, and no planner. -->
<Story name="DJ only" asChild>{@render dashboard(djOnly)}</Story>

<!-- Before there's a profile, the planner shows on its own. -->
<Story name="No profile yet" asChild>{@render dashboard(noProfile)}</Story>
