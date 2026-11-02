<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import CardPreview from './CardPreview.svelte';
	import SiteCard from './SiteCard.svelte';

	/** The link preview of the site's own pages, drawn at 1200×630 and shown here scaled to fit. */
	const { Story } = defineMeta({ title: 'Link previews/Site', component: SiteCard, tags: ['autodocs'] });

	const phone = { viewport: { value: 'mobile2', isRotated: false } };
</script>

<Story name="Site" asChild>
	<CardPreview><SiteCard /></CardPreview>
</Story>

<Story name="On a phone" asChild globals={phone}>
	<CardPreview><SiteCard /></CardPreview>
</Story>
