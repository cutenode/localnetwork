<script lang="ts">
	import type { Snippet } from 'svelte';
	import { clock, summarize, type MixReport } from '#lib/mix/report.ts';
	import { formatDate } from '#lib/format.ts';
	import KeyWheel from './KeyWheel.svelte';
	import MixTimeline from './MixTimeline.svelte';
	import MixTracklist from './MixTracklist.svelte';
	import TempoChart from './TempoChart.svelte';

	/** `details` adds lines under the mix's summary, like the set it was recorded at. */
	let { report, actions, details }: { report: MixReport; actions?: Snippet; details?: Snippet } = $props();

	const s = $derived(summarize(report));

	const byline = $derived([report.mix.artist, report.mix.genre, report.mix.date].filter(Boolean).join(' · '));

	let selected = $state<number | null>(null);

	const stats = $derived([
		{
			label: 'Tracks identified',
			value: String(s.tracks),
			sub: s.tracks ? `${s.confident} with high confidence` : 'none found'
		},
		{
			label: 'Of the mix identified',
			value: `${s.coverage}%`,
			sub: s.gaps ? `${s.gaps} stretch${s.gaps > 1 ? 'es' : ''} not in the library` : 'nothing missing'
		},
		{
			label: 'Tempo',
			value: s.tempo ? `${s.tempo.median.toFixed(0)} BPM` : '?',
			sub: s.tempo ? (s.tempo.steady ? 'held steady all set' : `${s.tempo.low.toFixed(0)}–${s.tempo.high.toFixed(0)} across the set`) : 'no BPMs in the library'
		},
		{
			label: 'Harmonic transitions',
			value: s.harmonic.known ? `${s.harmonic.good} of ${s.harmonic.known}` : '?',
			sub: s.harmonic.known ? 'by the keys as played' : 'keys unknown'
		}
	]);
</script>

<article class="space-y-6">
	<header class="card card-pad flex flex-col gap-5 sm:flex-row sm:items-end">
		<div class="size-40 shrink-0 border-2 border-ansi-cyan bg-ansi-blue">
			{#if report.mix.cover}
				<img src={report.mix.cover} alt="" class="size-full object-cover" />
			{:else}
				<div class="ansi-stripes size-full opacity-60" aria-hidden="true"></div>
			{/if}
		</div>
		<div class="min-w-0 flex-1 space-y-2">
			<p class="eyebrow text-sm">Mix</p>
			<h1 class="text-3xl break-words sm:text-4xl">{report.mix.title}</h1>
			{#if byline}<p class="text-lg text-ansi-white">{byline}</p>{/if}
			<p class="muted flex flex-wrap gap-x-4 text-sm">
				<span class="text-ansi-gray tabular-nums">{clock(report.mix.durationSecs)}</span>
				{#if report.mix.recorder}<span>Recorded on {report.mix.recorder}</span>{/if}
			</p>
			{#if details}{@render details()}{/if}
		</div>
		{#if actions}<div class="flex flex-wrap gap-2 sm:justify-end">{@render actions()}</div>{/if}
	</header>

	<dl class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
		{#each stats as stat (stat.label)}
			<div class="border-2 border-ansi-blue p-3">
				<dt class="text-ansi-bright-green">{stat.label}</dt>
				<dd class="text-4xl text-ansi-white tabular-nums">{stat.value}</dd>
				<dd class="text-sm text-ansi-cyan">{stat.sub}</dd>
			</div>
		{/each}
	</dl>

	<section class="card card-pad space-y-3">
		<h2 class="section-title">The mix</h2>
		<MixTimeline {report} bind:selected />
	</section>

	<div class="grid gap-6 lg:grid-cols-[3fr_2fr]">
		<section class="card card-pad space-y-3">
			<h2 class="section-title">Tempo</h2>
			<TempoChart {report} bind:selected />
		</section>
		<section class="card card-pad space-y-3">
			<h2 class="section-title">Key journey</h2>
			<KeyWheel {report} bind:selected />
		</section>
	</div>

	<section class="card card-pad space-y-3">
		<h2 class="section-title">Tracklist</h2>
		{#if report.plays.length}
			<MixTracklist {report} bind:selected />
		{:else}
			<p class="muted">No tracks from the library were found in this mix.</p>
		{/if}
	</section>

	<p class="muted text-sm">
		Found by {report.generator.name} {report.generator.version}, searching ±{Math.round(report.tempoRange * 100)}% tempo · analysed {formatDate(report.analysedAt)}
	</p>
</article>
