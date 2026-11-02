<script lang="ts">
	import ArtistPage from '#lib/components/supports/ArtistPage.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<ArtistPage supports={data.supports} />
