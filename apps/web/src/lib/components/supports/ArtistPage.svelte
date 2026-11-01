<script lang="ts">
	import { ArrowLeft } from '@lucide/svelte';
	import { page } from '$app/state';
	import type { ArtistSupports } from '#lib/types.ts';
	import ArtistSupport from './ArtistSupport.svelte';

	/** An artist's page, by MBID or by `name`. `supports` is null when the directory is offline. */
	let { supports }: { supports: ArtistSupports | null } = $props();
</script>

<div class="container-page flex flex-col gap-8 py-10 sm:py-14">
	<a href="/artists" class="inline-flex items-center gap-1.5 self-start text-small text-fg-muted no-underline hover:text-fg"><ArrowLeft class="size-4" />Artists</a>
	<ArtistSupport {supports} retryHref={page.url.pathname + page.url.search} />
</div>
