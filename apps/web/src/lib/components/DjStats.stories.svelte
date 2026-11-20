<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { DEMO_DJ_PROFILE } from '../../../scripts/seed-data.ts';
	import { djProfileSchema } from '#lib/dj-schema.ts';
	import { desktop, phone } from '#lib/testing/viewports.ts';
	import DjStats from './DjStats.svelte';

	const profile = djProfileSchema.parse(DEMO_DJ_PROFILE);

	/** Neon's playing stats from the seed data, as locallibrary publishes them. */
	const { Story } = defineMeta({ title: 'Patterns/DJ stats', component: DjStats, tags: ['autodocs'], args: { profile } });
</script>

<Story name="Every section" globals={desktop} />

<!-- A profile with only its tempo on file shows that and the credit line. -->
<Story
	name="Only the tempo"
	args={{ profile: djProfileSchema.parse({ generatedAt: '2026-10-01T00:00:00Z', tempo: { low: 120, high: 124, median: 122 } }) }}
	globals={desktop}
/>

<Story name="Phone" globals={phone} />
