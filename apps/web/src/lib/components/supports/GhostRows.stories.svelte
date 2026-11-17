<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import GhostRows from './GhostRows.svelte';

	/** The rows a supports list shows while it streams in. */
	const { Story } = defineMeta({ title: 'Supports/Ghost rows', component: GhostRows, tags: ['autodocs'] });
</script>

<Story name="Loading" args={{ label: 'Loading tracks' }} />

<Story name="Three rows" args={{ label: 'Loading artists', rows: 3 }} />
