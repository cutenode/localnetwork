<script lang="ts">
	let { score }: { score: number } = $props();

	const cells = Array.from({ length: 20 }, (_, index) => index);

	const filled = $derived(Math.round(Math.max(0, Math.min(100, score)) / 5));

	const tone = $derived(score >= 80 ? 'bg-ansi-bright-green' : score >= 60 ? 'bg-ansi-yellow' : 'bg-ansi-gray');
</script>

<div class="flex items-center gap-2" title="Match score {score} of 100">
	<div class="flex h-3.5 gap-px" aria-hidden="true">
		{#each cells as cell (cell)}<span class="w-1.5 {cell < filled ? tone : 'bg-ansi-blue'}"></span>{/each}
	</div>
	<span class="text-xl text-ansi-white tabular-nums">{score}</span>
</div>
