<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { INQUIRIES, promoterPage } from './fixtures.ts';
	import Page from './+page.svelte';

	/** Book DJs, the Promoter section's page: search, the matcher, the inquiries the viewer sent and their planned shows. */
	const { Story } = defineMeta({
		title: 'Patterns/Dashboard Book DJs',
		parameters: { sveltekit_experimental: { state: { page: { url: new URL('https://localnetwork.computer/dashboard/promoter') } } } }
	});

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const loading = new Promise<never>(() => {});
</script>

<Story name="Busy" asChild><Page params={{}} form={null} data={promoterPage({ inquiryCount: 9 })} /></Story>

<Story name="Busy on a phone" asChild globals={phone}><Page params={{}} form={null} data={promoterPage({ inquiryCount: 9 })} /></Story>

<!-- Just turned on promoting: nothing asked or planned yet. -->
<Story name="Nothing yet" asChild><Page params={{}} form={null} data={promoterPage({ inquiries: [], inquiryCount: 0, planner: Promise.resolve({ available: true, shows: [], total: 0 }) })} /></Story>

<Story name="Nothing yet on a phone" asChild globals={phone}><Page params={{}} form={null} data={promoterPage({ inquiries: [], inquiryCount: 0, planner: Promise.resolve({ available: true, shows: [], total: 0 }) })} /></Story>

<Story name="Planner loading" asChild><Page params={{}} form={null} data={promoterPage({ planner: loading })} /></Story>

<Story name="Planner unavailable" asChild><Page params={{}} form={null} data={promoterPage({ inquiries: INQUIRIES.slice(0, 1), inquiryCount: 1, planner: Promise.resolve({ available: false, problem: 'Your PDS did not respond' }) })} /></Story>
