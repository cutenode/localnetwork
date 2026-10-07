<script lang="ts">
	let { form }: { form: { error?: string; success?: string } | null | undefined } = $props();
</script>

{#if form?.error}
	<div role="alert" class="notice notice-error flex items-start gap-2">
		<span aria-hidden="true">!!</span>
		<span>{form.error}</span>
	</div>
{:else if form?.success}
	<div role="status" class="notice notice-ok flex items-start gap-2">
		<span aria-hidden="true">OK</span>
		<span>{form.success}</span>
	</div>
{/if}
