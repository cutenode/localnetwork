<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { phone } from '#lib/testing/viewports.ts';
	import NightsFailed from './NightsFailed.svelte';

	const { Story } = defineMeta({
		title: 'Dashboard/Nights failed',
		component: NightsFailed,
		tags: ['autodocs'],
		args: { href: '/dashboard/calendar' }
	});
</script>

<Story name="Some nights didn't load" />

<Story name="Phone" globals={phone} />
