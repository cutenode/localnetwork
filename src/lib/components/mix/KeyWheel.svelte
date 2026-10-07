<script lang="ts">
	import { keysPlayed, parseCamelot, type Camelot, type MixReport } from '#lib/mix/report.ts';

	let { report, selected = $bindable(null) }: { report: MixReport; selected?: number | null } = $props();

	const S = 360;

	const C = S / 2;

	const R_OUT = 150;

	const R_MID = 106;

	const R_IN = 62;

	const NODE = { major: 128, minor: 84 };

	const angle = (n: number) => ((n % 12) / 12) * Math.PI * 2 - Math.PI / 2;

	const pos = (k: Camelot, r = k.major ? NODE.major : NODE.minor) => [C + Math.cos(angle(k.n)) * r, C + Math.sin(angle(k.n)) * r];

	const used = $derived(keysPlayed(report));

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

	const cells = Array.from({ length: 12 }, (_, i) => i + 1);

	let focus = $state<string | null>(null);

	const focusPlays = $derived(focus ? (used.get(focus) ?? []) : []);
</script>

<svg viewBox="0 0 {S} {S}" class="mx-auto block w-full max-w-sm font-sans" role="img" aria-label="Keys as played, in order, on the Camelot wheel">
	{#each [R_OUT, R_MID, R_IN] as r (r)}
		<circle cx={C} cy={C} {r} class="fill-none stroke-ansi-blue" stroke-width="2" />
	{/each}
	{#each cells as n (n)}
		{@const a = angle(n) + Math.PI / 12}
		<line x1={C + Math.cos(a) * R_IN} y1={C + Math.sin(a) * R_IN} x2={C + Math.cos(a) * R_OUT} y2={C + Math.sin(a) * R_OUT} class="stroke-ansi-blue" stroke-width="2" />
		{#each [true, false] as major (major)}
			{@const id = `${n}${major ? 'B' : 'A'}`}
			{#if !used.has(id)}
				{@const [x, y] = pos({ n, major })}
				<text {x} y={y + 5} text-anchor="middle" font-size="15" class="fill-ansi-dark">{id}</text>
			{/if}
		{/each}
	{/each}
	{#each moves as m (m.key)}
		<path d={m.d} class="fill-none {m.clash ? 'stroke-ansi-bright-red' : 'stroke-ansi-bright-cyan'}" stroke-width="3" stroke-dasharray={m.clash ? '6 4' : undefined} />
	{/each}
	{#each [...used] as [id, plays] (id)}
		{@const k = parseCamelot(id)!}
		{@const [x, y] = pos(k)}
		{@const [lx, ly] = pos(k, k.major ? NODE.major + 24 : NODE.minor - 24)}
		<!-- svelte-ignore a11y_click_events_have_key_events -->
		<g
			role="button"
			tabindex="0"
			aria-label="{id}: track{plays.length > 1 ? 's' : ''} {plays.map((i) => i + 1).join(', ')}"
			class="cursor-pointer"
			onpointerenter={() => (focus = id)}
			onpointerleave={() => (focus = null)}
			onfocus={() => (focus = id)}
			onblur={() => (focus = null)}
			onclick={() => (selected = plays[0])}
		>
			<rect x={x - 14} y={y - 12} width="28" height="24" class={plays.includes(selected ?? -1) ? 'fill-ansi-yellow' : 'fill-ansi-magenta'} />
			<text {x} y={y + 6} text-anchor="middle" font-size="17" class={plays.includes(selected ?? -1) ? 'fill-ansi-black' : 'fill-ansi-white'}>{plays.length > 1 ? `${plays[0] + 1}+` : plays[0] + 1}</text>
			<text x={lx} y={ly + 5} text-anchor="middle" font-size="15" class="fill-ansi-yellow">{id}</text>
		</g>
	{/each}
	<text x={C} y={C - 2} text-anchor="middle" font-size="34" class="fill-ansi-white">{used.size}</text>
	<text x={C} y={C + 20} text-anchor="middle" font-size="15" class="fill-ansi-cyan">{used.size === 1 ? 'key' : 'keys'}</text>
</svg>
<div class="mt-2 flex flex-wrap gap-x-4 text-sm">
	<span class="flex items-center gap-1.5"><span class="inline-block h-1 w-4 bg-ansi-bright-cyan"></span>Harmonic move</span>
	<span class="flex items-center gap-1.5"><span class="inline-block h-1 w-4 bg-ansi-bright-red"></span>Clash</span>
</div>
<p class="mt-1 min-h-6 text-sm text-ansi-cyan" aria-live="polite">
	{#if focus}
		<span class="text-ansi-yellow">{focus}</span>: {focusPlays.map((i) => `${i + 1}. ${report.plays[i].track.title}`).join(', ')}
	{:else if used.size}
		Point at a key for the tracks heard in it.
	{:else}
		The library has no keys for these tracks.
	{/if}
</p>
