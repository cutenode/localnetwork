<script lang="ts">
	/** Placeholder rows in a card while a list streams in, read out once as `label`. */
	let { label, rows = 5 }: { label: string; rows?: number } = $props();
</script>

<div aria-busy="true" class="card flex flex-col divide-y divide-line">
	<span class="sr-only">{label}</span>
	{#each { length: rows }, i (i)}
		<div aria-hidden="true" class="flex items-center gap-3 px-4 py-3.5">
			<div class="h-3.5 w-5 animate-pulse rounded-sm bg-raised"></div>
			<div class="flex flex-1 flex-col gap-2">
				<div class="h-4 w-1/2 animate-pulse rounded-sm bg-raised"></div>
				<div class="h-3 w-1/3 animate-pulse rounded-sm bg-raised"></div>
			</div>
			<div class="h-3.5 w-12 animate-pulse rounded-sm bg-raised"></div>
		</div>
	{/each}
</div>
