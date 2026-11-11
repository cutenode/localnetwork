<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import type { FlagPreview } from '#lib/types.ts';
	import FlagPreviewBanner from './FlagPreviewBanner.svelte';

	/** Shown on every page while a staff member previews flags in their own browser. */
	const { Story } = defineMeta({ title: 'Staff/Flag preview banner', component: FlagPreviewBanner, tags: ['autodocs'] });

	// SAFETY: stories show flags the catalog may not have yet.
	const two = { newPlanner: true, parityCanary: false } as FlagPreview;
</script>

<Story name="One flag" args={{ preview: { parityCanary: true } }} />

<Story name="Two flags" args={{ preview: two }} />

<Story name="One flag, on a phone" args={{ preview: { parityCanary: false } }} globals={{ viewport: { value: 'mobile2', isRotated: false } }} />
