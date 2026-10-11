<script lang="ts">
	import { clock, colors, CONFIDENT, handoff, speed, tracklist, type MixReport } from '#lib/mix/report.ts';
	import TrackOverview from './TrackOverview.svelte';

	let { report, selected = $bindable(null) }: { report: MixReport; selected?: number | null } = $props();

	const items = $derived(tracklist(report));

	const handoffs = $derived(new Map(report.transitions.map((t) => [t.from, handoff(t)])));

	const cells = Array.from({ length: 10 }, (_, i) => i);

	const status = colors.status.dark;

	$effect(() => {
		if (selected === null) return;
		document.getElementById(`mix-track-${selected}`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
	});
</script>

<ol class="space-y-0">
	{#each items as item (item.kind === 'play' ? `p${item.index}` : `g${item.start}`)}
		{#if item.kind === 'gap'}
			<li class="border-b-2 border-dashed border-ansi-blue py-2 text-ansi-dark">
				?? {clock(item.start)} · {clock(item.end - item.start)} not found in the library
			</li>
		{:else}
			{@const play = item.play}
			{@const track = play.track}
			{@const i = item.index}
			{@const pitched = play.keyLock === false && Math.abs(play.semitones) >= 0.05}
			<li id="mix-track-{i}" class="scroll-mt-4 border-b-2 border-dashed border-ansi-blue py-3">
				<div
					role="button"
					tabindex="0"
					aria-pressed={selected === i}
					aria-label="{i + 1}. {track.title}{track.artist ? ` by ${track.artist}` : ''}, {clock(play.start)} to {clock(play.end)}"
					class="grid cursor-pointer grid-cols-[2.5rem_4rem_minmax(0,1fr)] gap-x-3 gap-y-2 border-l-4 pl-2 md:grid-cols-[2.5rem_4rem_minmax(0,1fr)_minmax(0,17rem)_7.5rem] {selected === i ? 'border-ansi-yellow' : 'border-transparent'}"
					onclick={() => (selected = selected === i ? null : i)}
					onkeydown={(event) => {
						if (event.key !== 'Enter' && event.key !== ' ') return;
						event.preventDefault();
						selected = selected === i ? null : i;
					}}
				>
					<span class="text-2xl text-ansi-yellow tabular-nums">{i + 1}</span>
					<span class="size-16 border-2 border-ansi-blue bg-ansi-blue/20">
						{#if track.artwork}<img src={track.artwork} alt="" class="size-full object-cover" loading="lazy" />{/if}
					</span>
					<div class="min-w-0 space-y-0.5">
						<p class="truncate text-lg text-ansi-white">{track.title}</p>
						{#if track.artist}<p class="truncate text-ansi-bright-cyan">{track.artist}</p>{/if}
						<p class="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-ansi-cyan">
							<span class="text-ansi-gray tabular-nums">{clock(play.start)}–{clock(play.end)}</span>
							{#if track.label}<span>{track.label}</span>{/if}
							{#if play.keyLock}<span class="chip">Key lock</span>{/if}
							{#if pitched}<span class="chip">Pitched {play.semitones > 0 ? '+' : '−'}{Math.abs(play.semitones).toFixed(1)} st</span>{/if}
							{#if play.jumps > 0}<span class="chip">{play.jumps} jump{play.jumps > 1 ? 's' : ''}/loop{play.jumps > 1 ? 's' : ''}</span>{/if}
							{#if play.confidence < CONFIDENT}<span class="chip" style:border-color={status.warning} style:color={status.warning}>Tentative</span>{/if}
						</p>
					</div>
					<div class="col-span-3 md:col-span-1"><TrackOverview {play} /></div>
					<div class="col-span-3 flex items-baseline gap-4 md:col-span-1 md:block md:text-right">
						<p class="text-2xl text-ansi-white tabular-nums">{play.playedBpm ? play.playedBpm.toFixed(1) : '?'} <span class="text-sm text-ansi-cyan">BPM</span></p>
						<p class="text-sm text-ansi-cyan tabular-nums">{track.bpm ? `${track.bpm.toFixed(1)} ` : ''}{speed(play.tempo)}</p>
						<p class="flex items-center gap-2 md:justify-end">
							<span class="text-ansi-yellow" title={track.key ? `${track.key} in rekordbox` : undefined}>
								{play.key && play.playedKey && play.key !== play.playedKey ? `${play.key} → ${play.playedKey}` : (play.playedKey ?? track.key ?? '?')}
							</span>
							<span class="flex h-2.5 gap-px" title="Confidence {Math.round(play.confidence * 100)}%" aria-label="Confidence {Math.round(play.confidence * 100)}%">
								{#each cells as cell (cell)}<span class="w-1 bg-ansi-blue" style:background-color={cell < Math.round(play.confidence * 10) ? status.good : undefined}></span>{/each}
							</span>
						</p>
					</div>
				</div>
			</li>
			{@const h = handoffs.get(i)}
			{#if h}
				<li class="flex flex-wrap items-center gap-x-4 border-b-2 border-dashed border-ansi-blue py-1 pl-14 text-sm" aria-label="Transition: {h.how}">
					<span class="text-ansi-gray">↓ {h.how}{h.tempo ? ` · ${h.tempo}` : ''}</span>
					{#if h.harmony}
						<span style:color={h.harmony.ok ? status.good : status.critical}>{h.harmony.ok ? `✓ ${h.harmony.text}` : `! ${h.harmony.text}`}</span>
					{/if}
				</li>
			{/if}
		{/if}
	{/each}
</ol>
