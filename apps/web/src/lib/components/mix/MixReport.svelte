<script lang="ts">
	import { ExternalLink, Pause, Play } from '@lucide/svelte';
	import type { Snippet } from 'svelte';
	import { formatDate, formatShowDate } from '#lib/format.ts';
	import { embedUrl, mixPlatform } from '#lib/mix/links.ts';
	import { clock, keyPath, summarize, type MixReport } from '#lib/mix/report.ts';
	import Button from '#lib/ui/Button.svelte';
	import Card from '#lib/ui/Card.svelte';
	import Monogram from '#lib/ui/Monogram.svelte';
	import KeyWheel from './KeyWheel.svelte';
	import MixEmbed from './MixEmbed.svelte';
	import MixTimeline from './MixTimeline.svelte';
	import MixTracklist from './MixTracklist.svelte';
	import TempoChart from './TempoChart.svelte';

	/**
	 * A mix report as a page: the header, then the timeline, tracklist, key moves and tempo, which
	 * share the selected track. `project` replaces the export's artist name (a link back to the
	 * profile), `listen` is the mix's link, played in place where the platform has a player.
	 */
	let { report, title, project, listen, actions }: { report: MixReport; title?: string; project?: Snippet; listen?: string; actions?: Snippet } = $props();

	const heading = $derived(title ?? report.mix.title);

	const s = $derived(summarize(report));

	const keys = $derived(keyPath(report));

	const embed = $derived(listen ? embedUrl(listen) : null);

	const platform = $derived(listen ? mixPlatform(listen) : null);

	const byline = $derived(
		[report.mix.date && (formatShowDate(report.mix.date) || report.mix.date), report.mix.recorder && `Recorded on ${report.mix.recorder}`, report.mix.genre].filter(Boolean).join(' · ')
	);

	const facts = $derived(
		[
			clock(report.mix.durationSecs),
			s.tempo && (s.tempo.steady ? `${s.tempo.median.toFixed(0)} BPM` : `${s.tempo.low.toFixed(0)}–${s.tempo.high.toFixed(0)} BPM`),
			keys.length && (keys.length > 4 ? `${keys[0]} → … → ${keys.at(-1)}` : keys.join(' → '))
		].filter(Boolean)
	);

	const stats = $derived([
		{ label: 'Tracks identified', value: String(s.tracks), sub: s.tracks ? `${s.confident} with high confidence` : 'none found' },
		{ label: 'Of the mix identified', value: `${s.coverage}%`, sub: s.gaps ? `${s.gaps} stretch${s.gaps > 1 ? 'es' : ''} not in the library` : 'nothing missing' },
		{ label: 'Harmonic transitions', value: s.harmonic.known ? `${s.harmonic.good} of ${s.harmonic.known}` : '?', sub: s.harmonic.known ? 'by the keys as played' : 'keys unknown' }
	]);

	let selected = $state<number | null>(null);

	let playing = $state(false);
</script>

<article class="flex flex-col gap-6 sm:gap-8">
	<header class="flex flex-wrap items-end justify-between gap-x-10 gap-y-5">
		<div class="flex min-w-0 flex-[1_1_30rem] items-end gap-5">
			{#if report.mix.cover}<img src={report.mix.cover} alt="" class="size-24 shrink-0 rounded-xl bg-well object-cover sm:size-32" />{/if}
			<div class="flex min-w-0 flex-col gap-3">
				{#if project}
					{@render project()}
				{:else if report.mix.artist}
					<p class="m-0 flex items-center gap-2.5 font-semibold"><Monogram name={report.mix.artist} size="sm" />{report.mix.artist}</p>
				{/if}
				<h1 class="m-0 text-title font-semibold break-words">{heading}</h1>
				<div class="flex flex-col gap-1">
					{#if byline}<p class="m-0 text-fg-muted">{byline}</p>{/if}
					<p class="m-0 flex flex-wrap gap-x-4 font-mono text-label text-fg-soft">{#each facts as fact (fact)}<span>{fact}</span>{/each}</p>
				</div>
			</div>
		</div>
		{#if listen || actions}
			<div class="flex flex-col gap-3 max-sm:w-full sm:flex-row sm:flex-wrap">
				{#if listen && embed}
					<Button aria-expanded={playing} aria-controls="mix-player" onclick={() => (playing = !playing)}>
						{#if playing}<Pause class="size-4" />Close player{:else}<Play class="size-4" />Play{/if}
					</Button>
				{/if}
				{#if listen}
					<Button variant={embed ? 'secondary' : 'primary'} href={listen} rel="noopener noreferrer nofollow" target="_blank">{embed ? 'Open' : 'Listen'} on {platform}<ExternalLink class="size-4" /></Button>
				{/if}
				{@render actions?.()}
			</div>
		{/if}
	</header>

	{#if embed && playing}<MixEmbed id="mix-player" src={embed} title={heading} />{/if}

	<Card padding="none" radius="2xl" class="overflow-hidden">
		<dl class="m-0 grid grid-cols-3 gap-px border-b border-line bg-line">
			{#each stats as stat (stat.label)}
				<div class="flex flex-col bg-surface px-3 py-3.5 sm:px-6 sm:py-4.5">
					<dt class="text-caption text-fg-muted sm:text-label">{stat.label}</dt>
					<dd class="m-0 font-mono text-emphasis font-semibold sm:text-heading">{stat.value}</dd>
					<dd class="m-0 text-caption text-fg-subtle max-sm:hidden">{stat.sub}</dd>
				</div>
			{/each}
		</dl>

		<section aria-labelledby="mix-timeline" class="flex flex-col gap-3 p-4 sm:p-6">
			<h2 id="mix-timeline" class="section-title m-0">Timeline</h2>
			<MixTimeline {report} bind:selected />
		</section>

		<div class="flex flex-wrap border-t border-line">
			<section aria-labelledby="mix-tracklist" class="min-w-0 flex-[2_1_36rem] border-line px-4 py-5 sm:border-r sm:px-6">
				<h2 id="mix-tracklist" class="section-title m-0 mb-2.5">Tracklist</h2>
				{#if report.plays.length || report.unidentified.length}
					<MixTracklist {report} bind:selected />
				{:else}
					<p class="m-0 text-fg-muted">No tracks from the library were found in this mix.</p>
				{/if}
			</section>
			<div class="flex min-w-0 flex-[1_1_18rem] flex-col">
				<section aria-labelledby="mix-keys" class="border-line px-4 py-5 max-sm:border-t sm:px-6">
					<h2 id="mix-keys" class="section-title m-0 mb-2.5">Key moves</h2>
					<KeyWheel {report} bind:selected />
				</section>
				<section aria-labelledby="mix-tempo" class="border-t border-line px-4 py-5 sm:px-6">
					<h2 id="mix-tempo" class="section-title m-0 mb-2.5">Tempo</h2>
					<TempoChart {report} bind:selected />
				</section>
			</div>
		</div>
	</Card>

	<p class="m-0 text-label text-fg-subtle">
		Found by {report.generator.name}
		{report.generator.version}, searching ±{Math.round(report.tempoRange * 100)}% tempo. Analysed {formatDate(report.analysedAt)}.
	</p>
</article>
