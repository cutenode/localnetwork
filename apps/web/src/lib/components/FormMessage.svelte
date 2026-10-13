<script lang="ts">
	import { CircleAlert, CircleCheck } from '@lucide/svelte';

	let { form }: { form: { error?: string; success?: string } | null | undefined } = $props();
</script>

{#if form?.error}
	<div role="alert" class="notice notice-error flex items-start gap-2.5">
		<CircleAlert aria-hidden="true" class="mt-0.5 size-4 shrink-0 text-critical" />
		<span>{form.error}</span>
	</div>
{:else if form?.success}
	<div role="status" class="notice notice-ok flex items-start gap-2.5">
		<CircleCheck aria-hidden="true" class="mt-0.5 size-4 shrink-0 text-good" />
		<span>{form.success}</span>
	</div>
{/if}
