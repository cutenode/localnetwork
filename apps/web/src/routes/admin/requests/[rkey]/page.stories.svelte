<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { ada, neon, neonTide, request } from '#lib/testing/requests.ts';
	import ReviewPage from './+page.svelte';

	/** The staff review of a change request whose avatar is a type the site won't show, which approval refuses. */
	const { Story } = defineMeta({ title: 'Patterns/Request review' });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };

	const data = {
		viewer: ada,
		pendingRequests: 1,
		request: request('update'),
		project: neonTide,
		listing: null,
		changes: [{ field: 'profile.avatar', after: 'image yuq3pkca' }],
		detailChanges: [],
		stale: false,
		managers: [neon],
		decided: null,
		history: [],
		trail: [],
		verification: null,
		avatar: { type: 'image/svg+xml', shown: false },
		tracklists: []
	};
</script>

<Story name="Avatar the site can't show" asChild><ReviewPage {data} params={{ rkey: '3kupdate' }} form={null} /></Story>

<Story name="Avatar the site can't show, on a phone" asChild globals={phone}><ReviewPage {data} params={{ rkey: '3kupdate' }} form={null} /></Story>
