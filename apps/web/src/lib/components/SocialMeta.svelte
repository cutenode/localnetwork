<script lang="ts">
	import { type PageMeta, SITE_NAME } from '#lib/meta.ts';
	import { jsonLdScript } from '#lib/structured-data.ts';

	/** The head tags for a public page: its title and description, canonical address, and what link previews show. */
	let { meta }: { meta: PageMeta } = $props();
</script>

<svelte:head>
	<title>{meta.documentTitle ?? `${meta.title} · ${SITE_NAME}`}</title>
	<!-- A script tag only {@html} can write, with what people wrote in it escaped by jsonLdScript. -->
	{#if meta.jsonLd}{@html jsonLdScript(meta.jsonLd)}{/if}
	<meta name="description" content={meta.description} />
	<link rel="canonical" href={meta.url} />
	{#if meta.noindex}<meta name="robots" content="noindex" />{/if}
	<meta property="og:site_name" content={SITE_NAME} />
	<meta property="og:type" content={meta.type} />
	<meta property="og:title" content={meta.title} />
	<meta property="og:description" content={meta.description} />
	<meta property="og:url" content={meta.url} />
	{#if meta.image}
		<meta property="og:image" content={meta.image.url} />
		<meta property="og:image:alt" content={meta.image.alt} />
		<meta name="twitter:image:alt" content={meta.image.alt} />
	{/if}
	<!-- Pictures so far are square avatars and photos, which the small card fits. -->
	<meta name="twitter:card" content="summary" />
</svelte:head>
