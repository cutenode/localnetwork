<script lang="ts">
	import Deck from '#lib/components/tools/deck/Deck.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<div class="container-page flex flex-col gap-10 py-10 sm:gap-14 sm:py-14">
	<div class="flex flex-col gap-4">
		<h1 class="m-0 text-display font-semibold text-balance">From the crate to the booth.</h1>
		<p class="m-0 max-w-2xl text-intro text-fg-muted">Free tools for DJs. Each one stands on its own, and locallibrary signs in with your Atmosphere account.</p>
	</div>
	<Deck tools={data.tools} track={data.track} />
</div>
