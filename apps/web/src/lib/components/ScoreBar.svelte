<script lang="ts">
	let { score }: { score: number } = $props();

	const width = $derived(Math.max(0, Math.min(100, score)));
</script>

<div class="flex items-center gap-2.5" title="Match score {score} of 100">
	<div class="h-1.5 w-20 overflow-hidden rounded-full bg-line" aria-hidden="true">
		<div class="h-full rounded-full bg-track-aqua" style:width="{width}%"></div>
	</div>
	<span class="font-mono text-small text-fg tabular-nums">{score}</span>
</div>
