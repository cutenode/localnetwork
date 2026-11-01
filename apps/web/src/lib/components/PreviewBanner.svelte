<script lang="ts">
	import { EyeOff } from '@lucide/svelte';
	import type { ProjectStatus } from '#lib/types.ts';

	/** Tells the managers and staff looking at a project that isn't public that visitors can't see this page. */
	let { status }: { status: ProjectStatus } = $props();
</script>

{#if status !== 'active'}
	<p class="notice notice-info m-0 flex items-center gap-2 print:hidden">
		<EyeOff class="size-4 shrink-0" aria-hidden="true" /><span><strong class="font-semibold">{status === 'archived' ? 'Archived.' : 'Not public yet.'}</strong> Only its managers and staff can see this page.</span>
	</p>
{/if}
