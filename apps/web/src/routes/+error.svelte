<script lang="ts">
	import { page } from '$app/state';
	import Button from '#lib/ui/Button.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';

	const title = $derived(page.status === 404 ? "We couldn't find that page" : page.status === 403 ? 'Not allowed' : page.status === 503 ? 'Directory offline' : 'Something went wrong');

	// SvelteKit's own 404 message is just "Not Found", which the title already says.
	const lede = $derived(page.status === 404 && page.error?.message === 'Not Found' ? 'The link may be out of date, or the page has moved.' : page.error?.message);
</script>

<svelte:head><title>{page.status} · Local Network</title></svelte:head>

<div class="container-page flex flex-col items-center gap-6 py-20 sm:py-30">
	<SectionHeader level={1} align="center" eyebrow="Error {page.status}" {title} {lede}>
		{#snippet actions()}<Button href="/">Back to home</Button><Button variant="secondary" href="/projects">Find DJs</Button>{/snippet}
	</SectionHeader>
	{#if page.error?.requestId}<p class="m-0 font-mono text-caption text-fg-subtle">Reference: {page.error.requestId}</p>{/if}
</div>
