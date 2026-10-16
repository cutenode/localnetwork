<script lang="ts">
	import { keysPlayed, parseCamelot, playColor, type Camelot, type MixReport } from '#lib/mix/report.ts';
	import { onColour } from '#lib/ui/contrast.ts';

	let { report, selected = $bindable(null) }: { report: MixReport; selected?: number | null } = $props();

	const S = 360;

	const C = S / 2;

	const NODE = { major: 140, minor: 92 };

	const R = 19;

	const angle = (n: number) => ((n % 12) / 12) * Math.PI * 2 - Math.PI / 2;

	const pos = (k: Camelot) => [C + Math.cos(angle(k.n)) * (k.major ? NODE.major : NODE.minor), C + Math.sin(angle(k.n)) * (k.major ? NODE.major : NODE.minor)];

	const used = $derived(keysPlayed(report));

	/** Each key in the colour of the first track heard in it. */
	const swatch = (plays: number[]) => onColour(playColor(plays[0]));

	const moves = $derived(
		report.transitions.flatMap((t) => {
			const a = parseCamelot(report.plays[t.from].playedKey);
			const b = parseCamelot(report.plays[t.to].playedKey);

			if (!a || !b || (a.n === b.n && a.major === b.major)) return [];
			const [x1, y1] = pos(a);
			const [x2, y2] = pos(b);
			const mx = (x1 + x2) / 2;
			const my = (y1 + y2) / 2;
			const qx = mx + (C - mx) * 0.35;
			const qy = my + (C - my) * 0.35;

			return [{ d: `M${x1} ${y1}Q${qx} ${qy} ${x2} ${y2}`, clash: t.harmony === 'clash', key: `${t.from}-${t.to}` }];
		})
	);

	/** Keys in the order they were heard, each move marked when it clashed. */
	const order = $derived(
		report.plays.flatMap((p, i) => {
			if (!p.playedKey) return [];
			const into = report.transitions.find((t) => t.to === i);

			return [{ i, key: p.playedKey, clash: into?.harmony === 'clash' }];
		})
	);

	const cells = Array.from({ length: 12 }, (_, i) => i + 1);

	let focus = $state<string | null>(null);

	const focusPlays = $derived(focus ? (used.get(focus) ?? []) : []);
</script>

{#if order.length}
	<ol aria-label="Keys in order" class="m-0 flex list-none flex-wrap items-center gap-1.5 p-0 sm:hidden">
		{#each order as step, k (step.i)}
			{@const tile = onColour(playColor(step.i))}
			<li class="flex items-center gap-1.5">
				{#if k}<span aria-hidden="true" class={step.clash ? 'text-critical' : 'text-fg-subtle'}>→</span>{#if step.clash}<span class="sr-only">clash,</span>{/if}{/if}
				<span class="rounded-xs px-1.5 font-mono text-caption" style:background-color={tile.background} style:color={tile.ink}>{step.key}</span>
			</li>
		{/each}
	</ol>
{:else}
	<p class="m-0 text-label text-fg-muted sm:hidden">The library has no keys for these tracks.</p>
{/if}

<div class="max-sm:hidden">
	<svg viewBox="0 0 {S} {S}" class="mx-auto block w-full max-w-72 font-mono" role="img" aria-label="Keys as played, in order, on the Camelot wheel">
		{#each cells as n (n)}
			{#each [true, false] as major (major)}
				{@const id = `${n}${major ? 'B' : 'A'}`}
				{#if !used.has(id)}
					{@const [x, y] = pos({ n, major })}
					<circle cx={x} cy={y} r={R} class="fill-well" />
					<text {x} y={y + 4.5} text-anchor="middle" font-size="13" class="fill-fg-subtle">{id}</text>
				{/if}
			{/each}
		{/each}
		{#each moves as m (m.key)}
			<path d={m.d} class={['fill-none', m.clash ? 'stroke-critical' : 'stroke-fg-muted']} stroke-width="2" stroke-dasharray={m.clash ? '5 4' : undefined} />
		{/each}
		{#each [...used] as [id, plays] (id)}
			{@const [x, y] = pos(parseCamelot(id)!)}
			{@const tile = swatch(plays)}
			{@const active = plays.includes(selected ?? -1)}
			<!-- svelte-ignore a11y_click_events_have_key_events -->
			<g
				role="button"
				tabindex="0"
				aria-label="{id}: track{plays.length > 1 ? 's' : ''} {plays.map((i) => i + 1).join(', ')}"
				aria-pressed={active}
				class="cursor-pointer outline-none"
				onpointerenter={() => (focus = id)}
				onpointerleave={() => (focus = null)}
				onfocus={() => (focus = id)}
				onblur={() => (focus = null)}
				onclick={() => (selected = plays[0])}
			>
				{#if active || focus === id}<circle cx={x} cy={y} r={R + 4} class="fill-none stroke-fg" stroke-width="2" />{/if}
				<circle cx={x} cy={y} r={R} style:fill={tile.background} />
				<text {x} y={y + 4.5} text-anchor="middle" font-size="13" font-weight="600" style:fill={tile.ink}>{id}</text>
			</g>
		{/each}
	</svg>
	<ul class="m-0 mt-3 flex list-none flex-wrap justify-center gap-x-4 p-0 text-label text-fg-muted">
		<li class="flex items-center gap-1.5"><span class="h-0.5 w-4 rounded-full bg-fg-muted"></span>Harmonic move</li>
		<li class="flex items-center gap-1.5"><span class="h-0.5 w-4 rounded-full bg-critical"></span>Clash</li>
	</ul>
	<p class="m-0 mt-1 min-h-10 text-center text-label text-fg-muted" aria-live="polite">
		{#if focus}
			<span class="font-mono text-fg">{focus}</span>: {focusPlays.map((i) => `${i + 1}. ${report.plays[i].track.title}`).join(', ')}
		{:else if used.size}
			{used.size === 1 ? '1 key' : `${used.size} keys`}. Point at one for the tracks heard in it.
		{:else}
			The library has no keys for these tracks.
		{/if}
	</p>
</div>
