<script module lang="ts">
	import type { ProfileTab, ProfileTabData } from './types.ts';

	export const tab: ProfileTab = {
		id: 'mixes',
		label: 'Mixes',
		order: 20,
		has: ({ profile }) => Boolean(profile?.record.performer?.mixes?.length)
	};
</script>

<script lang="ts">
	import MixList from '#lib/components/MixList.svelte';

	let { data }: { data: ProfileTabData } = $props();
</script>

<MixList mixes={data.profile?.record.performer?.mixes ?? []} sets={data.sets} />
