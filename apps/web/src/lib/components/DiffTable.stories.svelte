<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { phone } from '#lib/testing/viewports.ts';
	import DiffTable from './DiffTable.svelte';

	const { Story } = defineMeta({ title: 'Requests/Changes table', component: DiffTable, tags: ['autodocs'] });

	const changes = [
		{ field: 'profile.location.city', before: 'Leeds', after: 'York' },
		{ field: 'profile.description', after: 'Late-night jungle and a long run of records nobody else in the city owns on vinyl' },
		{ field: 'profile.travelRadiusKm', before: '200' }
	];
</script>

<Story name="Changes" args={{ changes }} />

<Story name="Changes, phone" args={{ changes }} globals={phone} />

<Story name="No changes" args={{ changes: [] }} />
