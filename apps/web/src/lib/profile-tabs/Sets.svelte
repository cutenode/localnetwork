<script module lang="ts">
	import type { ProfileTab, ProfileTabData } from './types.ts';

	export const tab: ProfileTab = {
		id: 'sets',
		label: 'Sets',
		order: 30,
		has: ({ dj }) => Boolean(dj?.some((account) => account.sets.length))
	};
</script>

<script lang="ts">
	import DjSets from '#lib/components/DjSets.svelte';

	let { data }: { data: ProfileTabData } = $props();
</script>

{#each data.dj ?? [] as account (account.did)}<DjSets sets={account.sets} />{/each}
