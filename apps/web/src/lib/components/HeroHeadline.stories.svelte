<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import HeroHeadline from './HeroHeadline.svelte';

	/** The homepage's headline, with its words from headline.json. */
	const { Story } = defineMeta({ title: 'Homepage/Hero headline', component: HeroHeadline, tags: ['autodocs'], args: { interval: 2200 } });
</script>

{#snippet headline()}<HeroHeadline />{/snippet}

<Story name="In the hero" asChild>
	<SectionHeader level={1} size="display" align="center" title={headline} lede="Promoters find DJs by sound, distance and budget. DJs keep a profile they own, with their booking details kept private." />
</Story>
