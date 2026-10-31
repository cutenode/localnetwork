<script lang="ts">
	import { countLabel, type DjProfile, trackBpm } from '#lib/dj.ts';
	import { formatDate } from '#lib/format.ts';
	import Tag from '#lib/ui/Tag.svelte';

	/**
	 * A DJ's playing stats from their locallibrary profile: tempo, how they mix, keys, energy, what
	 * they play most and their signature tracks. Every block only shows when its section is on file.
	 */
	let { profile }: { profile: DjProfile } = $props();

	const percentOf = (value: number | undefined) => (value === undefined ? null : `${value}%`);

	/** The tempo range on a 100 to 180 BPM scale, for the bar. */
	const tempoBar = $derived.by(() => {
		if (!profile.tempo) return null;

		const at = (bpm: number) => Math.min(Math.max(((bpm - 100) / 80) * 100, 0), 100);

		return { left: at(profile.tempo.low), width: Math.max(at(profile.tempo.high) - at(profile.tempo.low), 2), median: at(profile.tempo.median) };
	});

	const energy = $derived(
		profile.energy
			? [
					{ label: 'High', share: profile.energy.high, tone: 'bg-phrase-chorus' },
					{ label: 'Mid', share: profile.energy.mid, tone: 'bg-phrase-up' },
					{ label: 'Low', share: profile.energy.low, tone: 'bg-phrase-down' }
				]
			: []
	);

	const arc = $derived(
		profile.arc
			? [
					{ label: 'Build', share: profile.arc.builds ?? 0 },
					{ label: 'Hold steady', share: profile.arc.steady ?? 0 },
					{ label: 'Wind down', share: profile.arc.windsDown ?? 0 }
				].filter((part) => part.share > 0)
			: []
	);

	const mixing = $derived(
		[
			{ label: 'In key', value: profile.mixing?.harmonic, hint: 'Transitions into a key that mixes on the Camelot wheel' },
			{ label: 'Smooth tempo', value: profile.mixing?.smoothTempo, hint: 'Transitions that change tempo by 3% or less' },
			{ label: 'Genre changes', value: profile.mixing?.genreChanges, hint: 'Transitions into another genre' }
		].filter((row): row is { label: string; value: number; hint: string } => row.value !== undefined)
	);

	const lists = $derived(
		[
			{ title: 'Genres', items: profile.genres ?? [] },
			{ title: 'Labels', items: profile.labels ?? [] },
			{ title: 'Artists', items: profile.artists ?? [] }
		].filter((list) => list.items.length)
	);
</script>

<div class="flex flex-col gap-6">
	{#if profile.headline || profile.tags?.length}
		<div class="flex flex-col gap-3">
			{#if profile.headline}<p class="m-0 text-lede text-fg-soft">{profile.headline}</p>{/if}
			{#if profile.tags?.length}<div class="flex flex-wrap gap-1.5">{#each profile.tags as tag (tag.name)}<Tag>{tag.name}</Tag>{/each}</div>{/if}
		</div>
	{/if}

	<dl class="m-0 flex flex-wrap gap-px overflow-hidden rounded-xl border border-line bg-line">
		{#if profile.tempo && tempoBar}
			<div class="flex flex-[2_1_17rem] flex-col gap-2 bg-surface p-4">
				<dt class="eyebrow text-fg-subtle">Tempo</dt>
				<dd class="m-0 flex flex-col gap-2">
					<span class="font-mono text-heading">{profile.tempo.low}–{profile.tempo.high} <span class="text-small text-fg-muted">BPM</span></span>
					<span aria-hidden="true" class="relative h-1.5 rounded-full bg-line">
						<span class="absolute inset-y-0 rounded-full bg-band-low" style:left="{tempoBar.left}%" style:width="{tempoBar.width}%"></span>
						<span class="absolute -inset-y-1 w-0.5 rounded-full bg-playhead" style:left="{tempoBar.median}%"></span>
					</span>
					<span class="text-label text-fg-muted">Most plays fall here; the middle is {profile.tempo.median}</span>
				</dd>
			</div>
		{/if}
		{#if profile.sets}
			<div class="flex flex-[1_1_9rem] flex-col gap-1 bg-surface p-4">
				<dt class="eyebrow text-fg-subtle">Sets on file</dt>
				<dd class="m-0 font-mono text-heading">{countLabel(profile.sets.total) ?? '–'}</dd>
				<dd class="m-0 text-label text-fg-muted">{[profile.sets.since && `since ${profile.sets.since}`, profile.sets.medianTracks && `about ${profile.sets.medianTracks} tracks each`].filter(Boolean).join(', ')}</dd>
			</div>
		{/if}
		{#if profile.era?.medianYear}
			<div class="flex flex-[1_1_9rem] flex-col gap-1 bg-surface p-4">
				<dt class="eyebrow text-fg-subtle">Era</dt>
				<dd class="m-0 font-mono text-heading">{profile.era.medianYear}</dd>
				<dd class="m-0 text-label text-fg-muted">Half the plays came out by then{#if profile.era.recent !== undefined}; {profile.era.recent}% are from the last two years{/if}</dd>
			</div>
		{/if}
	</dl>

	{#if mixing.length || profile.keys?.top?.length}
		<div class="grid gap-6 sm:grid-cols-2">
			{#if mixing.length}
				<section aria-label="How they mix" class="flex flex-col gap-3">
					<h3 class="m-0 eyebrow text-fg-subtle">How they mix</h3>
					<dl class="m-0 flex flex-col gap-2.5">
						{#each mixing as row (row.label)}
							<div title={row.hint} class="grid grid-cols-[7rem_minmax(0,1fr)_2.5rem] items-center gap-3 text-small">
								<dt class="text-fg-soft">{row.label}</dt>
								<dd class="m-0 h-1.5 overflow-hidden rounded-full bg-line"><span class="block h-full rounded-full bg-track-aqua" style:width="{row.value}%"></span></dd>
								<dd class="m-0 text-right font-mono text-label">{row.value}%</dd>
							</div>
						{/each}
					</dl>
				</section>
			{/if}
			{#if profile.keys?.top?.length}
				<section aria-label="Keys" class="flex flex-col gap-3">
					<h3 class="m-0 eyebrow text-fg-subtle">Keys played most</h3>
					<p class="m-0 flex flex-wrap gap-1.5">{#each profile.keys.top as key (key.key)}<span class="rounded-xs bg-raised px-2 py-0.5 font-mono text-label">{key.key}</span>{/each}</p>
					{#if profile.keys.minor !== undefined}<p class="m-0 text-label text-fg-muted">{profile.keys.minor}% of plays in minor keys</p>{/if}
				</section>
			{/if}
		</div>
	{/if}

	{#if energy.length || arc.length}
		<div class="grid gap-6 sm:grid-cols-2">
			{#if energy.length}
				<section aria-label="Energy" class="flex flex-col gap-3">
					<h3 class="m-0 eyebrow text-fg-subtle">Energy</h3>
					<div aria-hidden="true" class="flex h-2 gap-0.5 overflow-hidden rounded-full">{#each energy as part (part.label)}<span class={part.tone} style:flex-grow={part.share}></span>{/each}</div>
					<p class="m-0 flex flex-wrap gap-x-4 text-label text-fg-muted">{#each energy as part (part.label)}<span>{part.label} {percentOf(part.share)}</span>{/each}</p>
				</section>
			{/if}
			{#if arc.length}
				<section aria-label="Set shape" class="flex flex-col gap-3">
					<h3 class="m-0 eyebrow text-fg-subtle">Set shape</h3>
					<p class="m-0 text-small text-fg-soft">How the last quarter of a set goes:</p>
					<p class="m-0 flex flex-wrap gap-x-4 text-label text-fg-muted">{#each arc as part (part.label)}<span>{part.label} {percentOf(part.share)}</span>{/each}</p>
				</section>
			{/if}
		</div>
	{/if}

	{#if lists.length}
		<div class="grid gap-6 sm:grid-cols-3">
			{#each lists as list (list.title)}
				<section aria-label="{list.title} played most" class="flex flex-col gap-2.5">
					<h3 class="m-0 eyebrow text-fg-subtle">{list.title}</h3>
					<ol class="m-0 flex list-none flex-col gap-2 p-0">
						{#each list.items as item (item.name)}
							<li class="flex flex-col gap-1">
								<span class="flex justify-between gap-3 text-small"><span class="min-w-0 truncate">{item.name}</span><span class="font-mono text-label text-fg-muted">{item.share}%</span></span>
								<span aria-hidden="true" class="h-1 overflow-hidden rounded-full bg-line"><span class="block h-full bg-fg-subtle" style:width="{item.share}%"></span></span>
							</li>
						{/each}
					</ol>
				</section>
			{/each}
		</div>
	{/if}

	{#if profile.signature?.length}
		<section aria-label="Signature tracks" class="flex flex-col gap-2.5">
			<h3 class="m-0 eyebrow text-fg-subtle">Signature tracks</h3>
			<ol class="m-0 list-none p-0">
				{#each profile.signature.slice(0, 8) as track, i (i)}
					<li class="grid grid-cols-[minmax(0,1fr)_auto_auto] items-baseline gap-3 border-t border-line py-2 text-small">
						<span class="min-w-0"><span class="font-semibold">{track.unidentified ? 'Unreleased' : (track.title ?? 'Untitled')}</span>{#if track.artist}{" "}<span class="text-fg-muted">{track.artist}</span>{/if}</span>
						<span class="font-mono text-label text-fg-muted">{trackBpm(track.bpmCentis) ?? ''}</span>
						<span class="w-9 text-center font-mono text-label">{track.key ?? ''}</span>
					</li>
				{/each}
			</ol>
		</section>
	{/if}

	<p class="m-0 text-caption text-fg-subtle">
		From locallibrary{profile.precision === 'rounded' ? ', rounded' : ''}, updated {formatDate(profile.generatedAt)}.
	</p>
</div>
