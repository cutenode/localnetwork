<script lang="ts">
	import { ArrowDown, AudioLines } from '@lucide/svelte';
	import { clock, CONFIDENT, handoff, playColor, speed, tracklist, type MixReport } from '#lib/mix/report.ts';
	import { onColour } from '#lib/ui/contrast.ts';
	import TrackOverview from './TrackOverview.svelte';

	/** `playing` is the play the player is at, by index, or -1. */
	let { report, selected = $bindable(null), playing = -1 }: { report: MixReport; selected?: number | null; playing?: number } = $props();

	const items = $derived(tracklist(report));

	const handoffs = $derived(new Map(report.transitions.map((t) => [t.from, handoff(t)])));

	const cells = Array.from({ length: 10 }, (_, i) => i);

	const grid = 'grid grid-cols-[2.75rem_minmax(0,1fr)_auto] gap-x-3 sm:grid-cols-[3.25rem_minmax(0,1fr)_minmax(0,13rem)_5.5rem] sm:gap-x-4';

	const toggle = (i: number) => (selected = selected === i ? null : i);

	$effect(() => {
		if (selected === null) return;
		document.getElementById(`mix-track-${selected}`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
	});
</script>

<ol class="m-0 list-none p-0">
	{#each items as item (item.kind === 'play' ? `p${item.index}` : `g${item.start}`)}
		{#if item.kind === 'gap'}
			<li class="{grid} items-baseline border-t border-well py-2.5 text-small">
				<span class="font-mono text-caption text-fg-subtle">{clock(item.start)}</span>
				<span class="text-fg-muted">Not in the library <span class="font-mono text-caption text-fg-subtle">· {clock(item.end - item.start)}</span></span>
			</li>
		{:else}
			{@const play = item.play}
			{@const track = play.track}
			{@const i = item.index}
			{@const pitched = play.keyLock === false && Math.abs(play.semitones) >= 0.05}
			{@const swatch = onColour(playColor(i))}
			{@const confidence = Math.round(play.confidence * 100)}
			{@const shifted = play.key && play.playedKey && play.key !== play.playedKey}
			<li id="mix-track-{i}" class="scroll-mt-20 border-t border-well" aria-current={playing === i || undefined}>
				<div
					role="button"
					tabindex="0"
					aria-pressed={selected === i}
					aria-label="{playing === i ? 'Playing now: ' : ''}{i + 1}. {track.title}{track.artist ? ` by ${track.artist}` : ''}, {clock(play.start)} to {clock(play.end)}"
					class={[grid, 'cursor-pointer items-start rounded-md py-2.5 transition-colors hover:bg-raised', selected === i && 'bg-well hover:bg-well']}
					style:box-shadow={selected === i ? `inset 3px 0 0 ${playColor(i)}` : undefined}
					onclick={() => toggle(i)}
					onkeydown={(event) => {
						if (event.key !== 'Enter' && event.key !== ' ') return;
						event.preventDefault();
						toggle(i);
					}}
				>
					<span class={['pl-2 font-mono text-caption leading-6', playing === i ? 'font-semibold text-fg' : 'text-fg-subtle']}>{clock(play.start)}</span>
					<div class="flex min-w-0 gap-3">
						{#if track.artwork}<img src={track.artwork} alt="" class="size-10 shrink-0 rounded-md bg-well object-cover" loading="lazy" />{/if}
						<div class="flex min-w-0 flex-col gap-1">
							<p class="m-0 text-small leading-6">
								{#if playing === i}<AudioLines class="mr-1 inline size-3.5 align-[-2px]" aria-hidden="true" />{/if}<span class="mr-1 font-mono text-caption text-fg-subtle">{i + 1}</span><span class="font-semibold">{track.title}</span>
								{#if track.artist}<span class="text-fg-muted max-sm:block"> {track.artist}</span>{/if}
							</p>
							{#if track.label || play.keyLock || pitched || play.jumps > 0 || play.confidence < CONFIDENT}
								<p class="m-0 flex flex-wrap gap-1.5 text-caption">
									{#if track.label}<span class="text-fg-subtle">{track.label}</span>{/if}
									{#if play.keyLock}<span class="pill bg-raised text-fg-muted">Key lock</span>{/if}
									{#if pitched}<span class="pill bg-raised text-fg-muted">Pitched {play.semitones > 0 ? '+' : '−'}{Math.abs(play.semitones).toFixed(1)} st</span>{/if}
									{#if play.jumps > 0}<span class="pill bg-raised text-fg-muted">{play.jumps} {play.jumps > 1 ? 'jumps or loops' : 'jump or loop'}</span>{/if}
									{#if play.confidence < CONFIDENT}<span class="pill bg-warning/10 text-warning">Tentative</span>{/if}
								</p>
							{/if}
						</div>
					</div>
					<div class="pt-1 max-sm:hidden"><TrackOverview {play} /></div>
					<div class="flex flex-col items-end gap-1 text-right">
						<span class="rounded-xs px-1.5 font-mono text-caption leading-6" style:background-color={swatch.background} style:color={swatch.ink} title={track.key ? `${track.key} in rekordbox` : undefined}>{play.playedKey ?? track.key ?? '?'}</span>
						{#if shifted}<span class="font-mono text-caption text-fg-subtle">from {play.key}</span>{/if}
						<span class="font-mono text-small max-sm:hidden">{play.playedBpm ? play.playedBpm.toFixed(1) : '?'} <span class="text-caption text-fg-subtle">BPM</span></span>
						<span class="font-mono text-caption text-fg-subtle max-sm:hidden" title={track.bpm ? `${track.bpm.toFixed(1)} BPM as recorded` : undefined}>{speed(play.tempo)}</span>
						<span class="flex h-2 gap-px max-sm:hidden" role="img" title="Confidence {confidence}%" aria-label="Confidence {confidence}%">
							{#each cells as cell (cell)}<span class={['w-1 rounded-[1px]', cell < Math.round(play.confidence * 10) ? 'bg-good' : 'bg-line-strong']}></span>{/each}
						</span>
					</div>
				</div>
			</li>
			{@const h = handoffs.get(i)}
			{#if h}
				<li class="flex flex-wrap items-center gap-x-2 pb-2 pl-[3.75rem] font-mono text-caption text-fg-subtle sm:pl-[4.25rem]" aria-label="Transition: {h.how}{h.tempo ? `, ${h.tempo}` : ''}{h.harmony ? `, ${h.harmony.text}` : ''}">
					<ArrowDown class="size-3" aria-hidden="true" />
					<span>{h.how}{h.tempo ? ` · ${h.tempo}` : ''}</span>
					{#if h.harmony}<span class={h.harmony.ok ? 'text-good' : 'text-critical'}>{h.harmony.text}</span>{/if}
				</li>
			{/if}
		{/if}
	{/each}
</ol>
