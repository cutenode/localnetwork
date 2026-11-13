<script module lang="ts">
	import type { ProfileTab, ProfileTabData } from './types.ts';

	export const tab: ProfileTab = {
		id: 'how-they-play',
		label: 'How they play',
		order: 40,
		has: ({ dj }) => Boolean(dj?.some((account) => account.profile))
	};
</script>

<script lang="ts">
	import DjStats from '#lib/components/DjStats.svelte';

	let { data }: { data: ProfileTabData } = $props();
</script>

{#each data.dj ?? [] as account (account.did)}
	{#if account.profile}<DjStats profile={account.profile} />{/if}
{/each}
