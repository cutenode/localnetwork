<script lang="ts">
	import { ScrollText } from '@lucide/svelte';
	import FlagTable from '#lib/components/FlagTable.svelte';
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<svelte:head><title>Flags · Local Network</title></svelte:head>

<div class="flex flex-col gap-6">
	<SectionHeader level={1} title="Flags" lede="Turn a feature on for single accounts, groups, a share of accounts or every account, then for signed-out visitors last. The website and locallibrary give an account the same answer.">
		<!-- Saves only: a preview changes one browser, not who has the flag. -->
		{#snippet actions()}<a class="btn-secondary" href="/admin/audit?action=flag.update"><ScrollText class="size-4" />Recent changes</a>{/snippet}
	</SectionHeader>
	<FlagTable flags={data.flags} />
</div>
