<script lang="ts">
	import BookingHome from '#lib/components/home/BookingHome.svelte';
	import SocialHome from '#lib/components/home/SocialHome.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

{#if data.social}<SocialHome {data} />{:else}<BookingHome {data} />{/if}
