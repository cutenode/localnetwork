<script lang="ts">
	import { page } from '$app/state';
</script>

<svelte:head><title>{page.status} · Local Network</title></svelte:head>

<div class="container-page py-24 text-center">
	<p class="eyebrow">Error {page.status}</p>
	<h1 class="mt-3 text-4xl">{page.status === 404 ? "We couldn't find that page" : page.status === 403 ? 'Not allowed' : page.status === 503 ? 'Directory offline' : 'Something went wrong'}</h1>
	<p class="muted mx-auto mt-4 max-w-lg">{page.error?.message}</p>
	{#if page.error?.requestId}<p class="mono muted mt-2">Reference: {page.error.requestId}</p>{/if}
	<a href="/" class="btn-primary mt-8">Back to the start</a>
</div>
