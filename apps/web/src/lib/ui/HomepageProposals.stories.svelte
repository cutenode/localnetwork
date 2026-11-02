<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { clock, colors } from '#lib/mix/report.ts';
	import { sampleReport } from '#lib/mix/sample.ts';
	import { locallibrary } from '#lib/tools/locallibrary.ts';
	import { localtoolbox } from '#lib/tools/localtoolbox.ts';
	import type { ShowRecord } from '#lib/types.ts';
	import BrowserFrame from './BrowserFrame.svelte';
	import Button from './Button.svelte';
	import Card from './Card.svelte';
	import { onColour } from './contrast.ts';
	import CodeBlock from './CodeBlock.svelte';
	import DateBlock from './DateBlock.svelte';
	import Eyebrow from './Eyebrow.svelte';
	import Monogram from './Monogram.svelte';
	import Section from './Section.svelte';
	import SectionHeader from './SectionHeader.svelte';
	import Tag from './Tag.svelte';
	import Waveform from './Waveform.svelte';

	/**
	 * Proposals: homepage sections the site doesn't have, drawn with the design system. None of these
	 * are live. The live homepage is Patterns/Homepage. The mix is the sample mix report; the shows,
	 * matches and tool previews are examples.
	 */
	const { Story } = defineMeta({ title: 'Proposals/Homepage', parameters: { layout: 'fullscreen' } });

	const mix = sampleReport;

	const duration = mix.mix.durationSecs;

	const plays = mix.plays.map((play, i) => ({
		title: play.track.title,
		artist: play.track.artist,
		bpm: play.track.bpm,
		key: play.key,
		at: clock(play.start),
		color: [colors.trackColors.blue, colors.trackColors.aqua, colors.trackColors.yellow, colors.trackColors.orange, colors.trackColors.pink][i % 5],
		left: `${(play.start / duration) * 100}%`,
		width: `${((play.end - play.start) / duration) * 100}%`,
		row: i % 2
	})).map((play) => ({ ...play, swatch: onColour(play.color) }));

	const keyColor = new Map(plays.map((p) => [p.key, p.swatch]));

	const wheel = Array.from({ length: 12 }, (_, i) => i + 1).flatMap((k) => {
		const a = ((k - 1) * 30 - 90) * (Math.PI / 180);

		return (
			[
				['B', 42],
				['A', 26]
			] as const
		).map(([ring, r]) => ({ label: `${k}${ring}`, x: `${50 + r * Math.cos(a)}%`, y: `${50 + r * Math.sin(a)}%`, color: keyColor.get(`${k}${ring}`) }));
	});

	const tempo = Array.from({ length: 43 }, (_, i) => {
		const play = mix.plays.findLast((p) => i * 40 >= p.start) ?? mix.plays[0];

		return `${25 + ((play.track.bpm ?? 124) - 124) * 18}%`;
	});

	const phrases = [2, 3, 4, 2, 3, 4, 2, 3, 4, 2].map((grow, i) => ({ grow, color: [colors.phrases.intro, colors.phrases.up, colors.phrases.chorus, colors.phrases.down][i % 4] }));

	const project = (rkey: string) => `at://did:plc:root/computer.localnetwork.project/${rkey}`;

	const event = (rkey: string) => ({ uri: `at://did:plc:neon/community.lexicon.calendar.event/${rkey}`, cid: 'bafyreie5737gdxlw5i64vzichcalba3z2v5n6icifvx5xytvske7mr3hpm' });

	/** Upcoming shows as their computer.localnetwork.show records, with the name each project's card shows. */
	const shows: { name: string; slug: string; record: ShowRecord }[] = [
		{
			name: 'Neon Tide',
			slug: 'neon-tide',
			record: { project: project('neon-tide'), event: event('3m2tidal'), name: 'Tidal Hours', startsAt: '2026-10-16T23:00:00+02:00', endsAt: '2026-10-17T06:00:00+02:00', venue: 'Halle Nord', location: { city: 'Berlin', region: 'Berlin', country: 'DE', latE6: 52536000, lngE6: 13370000 }, tickets: 'https://tickets.example/tidal-hours', createdAt: '2026-10-01T12:00:00Z' }
		},
		{
			name: 'Saffron Social',
			slug: 'saffron-social',
			record: { project: project('saffron-social'), event: event('3m2roof'), name: 'Saffron Social on the roof', startsAt: '2026-10-17T15:00:00+02:00', endsAt: '2026-10-17T22:00:00+02:00', venue: 'Klunkerkranich', location: { city: 'Berlin', region: 'Berlin', country: 'DE', latE6: 52481300, lngE6: 13432200 }, createdAt: '2026-10-02T12:00:00Z' }
		},
		{
			name: 'Lowkey Lagos',
			slug: 'lowkey-lagos',
			record: { project: project('lowkey-lagos'), event: event('3m2arches'), name: 'Lowkey Lagos presents', startsAt: '2026-10-17T22:00:00+01:00', endsAt: '2026-10-18T04:00:00+01:00', venue: 'Arches Yard', location: { city: 'London', country: 'GB' }, createdAt: '2026-10-03T12:00:00Z' }
		}
	];

	/** The venue's clock: a show's times carry its UTC offset, so their first characters read as local time. */
	const hours = (record: ShowRecord) => (record.endsAt ? `${record.startsAt.slice(11, 16)} to ${record.endsAt.slice(11, 16)}` : `${record.startsAt.slice(11, 16)} till late`);

	const record = JSON.stringify(shows[0].record, null, 2);

	/** The matcher's factors and weights, in the order the match page lists them (matchProjects.lua). */
	const factors = [
		['sound', 40],
		['distance', 25],
		['budget', 15],
		['room', 8],
		['slot', 7],
		['setup', 5]
	] as const;

	const matches = [
		{ name: 'Saffron Social', slug: 'saffron-social', why: 'Disco and edits, 3 km away, in budget', bars: [95, 90, 100, 85, 100, 100] },
		{ name: 'Halflight', slug: 'halflight', why: 'Deep house, 27 km away, small rooms', bars: [70, 60, 100, 100, 50, 100] },
		{ name: 'Neon Tide', slug: 'neon-tide', why: 'House adjacent, 2 km away, top of budget', bars: [50, 100, 45, 90, 100, 60] }
	];

	/** The score the matcher would give: each factor times its weight. */
	const fit = (bars: number[]) => Math.round(bars.reduce((sum, bar, i) => sum + bar * factors[i][1], 0) / 100);

	const session = ['$ locallibrary check', '✓ USB-A     up to date', '! USB-B     2 playlists behind, syncing', '✓ Ready for Tidal Hours, Fri 16 Oct'].join('\n');

	const queue = [
		{ title: 'Night Bus', artist: 'Ferrule', status: 'free download', tone: 'text-good' },
		{ title: 'Halogen', artist: 'Two Rooms', status: 'gate: follow', tone: 'text-warning' },
		{ title: 'Static Bloom', artist: 'Kestrel Unit', status: 'downloaded', tone: 'text-fg-subtle' },
		{ title: 'Signal Path', artist: 'Mira Okonkwo', status: 'buy link', tone: 'text-link' }
	];
</script>

{#snippet lanes()}
	<!-- On a phone the lanes are colour bars; the tracklist names them. -->
	<div class="relative h-7 sm:h-14">
		{#each plays as play (play.title)}
			<span
				title={play.title}
				class={['absolute h-3 truncate rounded-sm px-2 text-caption leading-6 font-semibold sm:h-6', play.row ? 'top-4 sm:top-7.5' : 'top-0']}
				style:left={play.left}
				style:width={play.width}
				style:background-color={play.swatch.background}
				style:color={play.swatch.ink}><span class="max-sm:hidden">{play.title}</span></span
			>
		{/each}
	</div>
{/snippet}

<!-- A product shot under the hero: a project page with its latest mix. The live hero has the search form instead. -->
<Story name="Hero preview" asChild>
	<Section labelledby="preview-title">
		<h2 id="preview-title" class="sr-only">A project page</h2>
		<BrowserFrame url="localnetwork.computer/projects/neon-tide" bleed>
			<div class="flex flex-wrap">
				<div class="flex min-w-0 flex-[1_1_20rem] flex-col gap-4 border-line p-5 sm:border-r sm:p-7">
					<div class="flex items-center gap-3.5"><Monogram name="Neon Tide" size="xl" /><div><p class="m-0 text-heading font-semibold">Neon Tide</p><p class="m-0 text-small text-fg-muted">Solo DJ · Berlin, DE · since 2014</p></div></div>
					<p class="m-0 text-fg-soft">Rolling, hypnotic techno for late rooms.</p>
					<div class="flex flex-wrap gap-1.5 max-sm:hidden"><Tag>techno</Tag><Tag>dub techno</Tag><Tag>hypnotic techno</Tag></div>
					<dl class="m-0 grid grid-cols-[auto_1fr] gap-x-5 gap-y-1.5 text-small max-sm:hidden">
						<dt class="text-fg-subtle">Fee</dt><dd class="m-0 font-mono text-label">€600–1,500</dd>
						<dt class="text-fg-subtle">Rooms</dt><dd class="m-0 font-mono text-label">150–1,500 cap</dd>
						<dt class="text-fg-subtle">Next</dt><dd class="m-0">Tidal Hours, Fri 16 Oct, Halle Nord</dd>
					</dl>
					<div class="mt-auto flex gap-2 max-sm:hidden"><Button size="sm" href="/dashboard/bookings/new?project=neon-tide">Send inquiry</Button><Button size="sm" variant="secondary">Save</Button></div>
				</div>
				<div class="flex min-w-0 flex-[2_1_30rem] flex-col gap-3.5 border-line p-5 max-sm:border-t sm:p-7">
					<div class="flex flex-wrap items-baseline justify-between gap-2"><span class="font-semibold">{mix.mix.title}</span><span class="font-mono text-caption text-fg-subtle">{mix.plays.length} tracks · {clock(duration)} · 124–128 BPM</span></div>
					<Waveform bands={mix.waveform} playhead={0.38} label="{mix.mix.title}, {clock(duration)}" class="h-20 sm:h-30" />
					{@render lanes()}
				</div>
			</div>
		</BrowserFrame>
	</Section>
</Story>

<!-- A mix report on the homepage: waveform, tracklist, key moves and tempo. Mixes show on project pages today, but the homepage has no mix section. -->
<Story name="Mixes" asChild>
	<Section tone="sunken" labelledby="mixes-title">
		<div class="flex flex-col gap-8 sm:gap-12">
			<SectionHeader id="mixes-title" align="split" eyebrow="Mixes" tone="mid" title="Every mix, down to the transition." lede="Export a set from locallibrary and the profile shows its waveform, tracklist, tempo, key moves and every blend. Straight from your player history, no typing." />
			<Card padding="none" radius="2xl" class="overflow-hidden">
				<div class="flex flex-wrap items-center justify-between gap-3 border-b border-line px-4 py-4 sm:px-6 sm:py-4.5">
					<div class="flex items-center gap-3.5">
						<button type="button" aria-label="Play {mix.mix.title}" class="grid size-10 place-items-center rounded-full bg-fg text-fg-inverse"><svg aria-hidden="true" viewBox="0 0 12 14" class="ml-0.5 size-3.5 fill-current"><path d="M0 0l12 7-12 7z" /></svg></button>
						<div><p class="m-0 font-semibold">{mix.mix.title}</p><p class="m-0 text-label text-fg-muted">Saffron Social · Berlin</p></div>
					</div>
					<p class="m-0 flex gap-5 font-mono text-caption text-fg-muted"><span>{clock(duration)}</span><span>124–128 BPM</span><span>8A → 3B</span></p>
				</div>
				<div class="flex flex-col gap-3 p-4 sm:p-6">
					<div aria-hidden="true" class="flex h-2.5 gap-0.75">{#each phrases as phrase, i (i)}<span class="rounded-[2px]" style:flex-grow={phrase.grow} style:background-color={phrase.color}></span>{/each}</div>
					<Waveform bands={mix.waveform} label="{mix.mix.title}, {clock(duration)}" class="h-24 sm:h-35" />
					{@render lanes()}
				</div>
				<div class="flex flex-wrap border-t border-line">
					<div class="min-w-0 flex-[2_1_30rem] border-line px-4 py-5 sm:border-r sm:px-6">
						<Eyebrow>Tracklist</Eyebrow>
						<ol class="m-0 mt-2.5 list-none p-0">
							{#each plays as play (play.title)}
								<li class="grid grid-cols-[2.75rem_minmax(0,1fr)_2.75rem] items-baseline gap-3 border-t border-well py-2 text-small sm:grid-cols-[3.25rem_minmax(0,1fr)_3.5rem_2.75rem]">
									<span class="font-mono text-caption text-fg-subtle">{play.at}</span>
									<span class="min-w-0"><span class="font-semibold">{play.title}</span> <span class="text-fg-muted max-sm:block">{play.artist}</span></span>
									<span class="text-right font-mono text-caption max-sm:hidden">{play.bpm}</span>
									<span class="rounded-xs text-center font-mono text-caption" style:background-color={play.swatch.background} style:color={play.swatch.ink}>{play.key}</span>
								</li>
							{/each}
						</ol>
					</div>
					<div class="min-w-0 flex-[1_1_16rem] border-line px-4 py-5 max-sm:border-t sm:border-r sm:px-6">
						<Eyebrow>Key moves</Eyebrow>
						<ol aria-label="Keys in order" class="m-0 mt-2.5 flex list-none flex-wrap items-center gap-1.5 p-0 sm:hidden">{#each plays as play, i (play.title)}<li class="flex items-center gap-1.5">{#if i}<span aria-hidden="true" class="text-fg-subtle">→</span>{/if}<span class="rounded-xs px-1.5 font-mono text-caption" style:background-color={play.swatch.background} style:color={play.swatch.ink}>{play.key}</span></li>{/each}</ol>
						<div role="img" aria-label="Camelot wheel: 8A, 9A, 10A, then 3B" class="relative mx-auto mt-2.5 aspect-square w-55 max-w-full max-sm:hidden">
							{#each wheel as key (key.label)}
								<span aria-hidden="true" class={['absolute -mt-3.75 -ml-3.75 grid size-7.5 place-items-center rounded-full font-mono text-[10px]', !key.color && 'bg-well text-fg-subtle']} style:left={key.x} style:top={key.y} style:background-color={key.color?.background} style:color={key.color?.ink}>{key.label}</span>
							{/each}
						</div>
					</div>
					<div class="min-w-0 flex-[1_1_16rem] border-line px-4 py-5 max-sm:border-t sm:px-6">
						<Eyebrow>Tempo</Eyebrow>
						<div aria-hidden="true" class="mt-2.5 flex h-24 items-end gap-0.5 border-b border-line-strong sm:h-37.5">{#each tempo as h, i (i)}<span class="flex-1 border-t-2 border-band-high bg-line-strong" style:height={h}></span>{/each}</div>
						<p class="m-0 mt-1.5 flex justify-between font-mono text-caption text-fg-subtle"><span>124</span><span>126</span><span>128 BPM</span></p>
					</div>
				</div>
			</Card>
		</div>
	</Section>
</Story>

<!-- Upcoming shows on the homepage, from computer.localnetwork.show records. They're listed at /shows today, but not on the homepage. -->
<Story name="Shows" asChild>
	<Section labelledby="shows-title">
		<div class="flex flex-wrap gap-8 sm:gap-14">
			<div class="flex min-w-0 flex-[1_1_22rem] flex-col gap-6">
				<SectionHeader id="shows-title" eyebrow="Shows" tone="chorus" title="See who's playing, then book them." lede="Nights near you, from the DJs' own records." />
				<!-- The record is a detail for wider screens; on a phone the shows come first. -->
				<div class="max-sm:hidden"><CodeBlock language="json" code={record} title="computer.localnetwork.show" meta="public record" /></div>
			</div>
			<div class="flex min-w-0 flex-[1_1_34rem] flex-col gap-3">
				<div class="flex flex-wrap items-center justify-between gap-2.5">
					<h3 class="m-0 text-body font-semibold">Next 7 days</h3>
					<a href="/shows" class="text-small font-semibold text-link no-underline">All shows →</a>
				</div>
				<ol class="m-0 flex list-none flex-col gap-2.5 p-0">
					{#each shows as show (show.record.name)}
						<Card as="li" padding="sm" class="flex items-start gap-x-4 sm:flex-wrap sm:items-center sm:gap-x-5 sm:gap-y-3">
							<DateBlock date={show.record.startsAt.slice(0, 10)} />
							<div class="flex min-w-0 flex-[1_1_14rem] flex-col gap-2">
								<div>
									<p class="m-0 text-lede font-semibold">{show.record.name}</p>
									<p class="m-0 text-small text-fg-muted">{show.record.venue}, {show.record.location.city} · {hours(show.record)}</p>
								</div>
								<p class="m-0 flex items-center gap-2.5 text-small"><Monogram name={show.name} size="sm" /><a href="/projects/{show.slug}" class="no-underline">{show.name}</a></p>
								<a href="/dashboard/bookings/new?project={show.slug}" class="text-small font-semibold text-link no-underline sm:hidden">Book {show.name} →</a>
							</div>
							<Button size="sm" variant="secondary" href="/dashboard/bookings/new?project={show.slug}" class="ml-auto max-sm:hidden">Book {show.name}</Button>
						</Card>
					{/each}
				</ol>
			</div>
		</div>
	</Section>
</Story>

<!-- The matcher on the homepage: a sample show and its top three, scored on the six factors the matcher weighs. The live section links to /match instead. -->
<Story name="Match demo" asChild>
	<Section tone="sunken" labelledby="match-title">
		<div class="flex flex-col gap-8 sm:gap-12">
			<SectionHeader id="match-title" eyebrow="For promoters" tone="aqua" title="Describe the night. Get a shortlist with reasons." />
			<div class="flex flex-wrap gap-4">
				<Card padding="lg" radius="2xl" class="flex min-w-0 flex-[1_1_20rem] flex-col gap-3 max-sm:p-4">
					<Eyebrow>Your show</Eyebrow>
					<dl class="m-0 flex flex-col gap-2 max-sm:flex-row max-sm:flex-wrap">
						{#each [['Date', 'Sat 17 Oct'], ['Venue', 'Kreuzberg, Berlin'], ['Sound', 'disco, house'], ['Room', '300 cap'], ['Budget', 'Established'], ['Slot', 'Peak time'], ['Setup', 'Vinyl']] as [term, value] (term)}
							<div class="flex justify-between gap-3 rounded-md bg-well px-3 py-2.5 text-small max-sm:rounded-full max-sm:py-1.5"><dt class="text-fg-muted max-sm:sr-only">{term}</dt><dd class="m-0 font-medium">{value}</dd></div>
						{/each}
					</dl>
					<Button href="/match">Match DJs</Button>
				</Card>
				<ol class="m-0 flex min-w-0 flex-[2_1_34rem] list-none flex-col gap-3 p-0">
					{#each matches as match (match.name)}
						<Card as="li" padding="md" radius="2xl" class="flex flex-wrap items-center gap-x-7 gap-y-3 max-sm:p-4">
							<div class="flex min-w-0 flex-[1_1_14rem] items-center gap-3"><Monogram name={match.name} /><div><a href="/projects/{match.slug}" class="text-emphasis font-semibold no-underline">{match.name}</a><p class="m-0 text-label text-fg-muted">{match.why}</p></div></div>
							<p class="m-0 flex w-full items-center gap-2.5 font-mono text-caption text-fg-subtle sm:hidden"><span class="h-1.5 flex-1 overflow-hidden rounded-full bg-line"><span class="block h-full bg-track-aqua" style:width="{fit(match.bars)}%"></span></span>{fit(match.bars)} fit</p>
							<dl class="m-0 grid min-w-0 flex-[1_1_17rem] grid-cols-[3.75rem_minmax(0,1fr)_3.75rem_minmax(0,1fr)] items-center gap-x-2.5 gap-y-1.5 font-mono text-caption text-fg-subtle max-sm:hidden">
								{#each factors as [factor], i (factor)}<dt>{factor}</dt><dd class="m-0 h-1.5 overflow-hidden rounded-full bg-line"><span class="sr-only">{match.bars[i]} of 100</span><span aria-hidden="true" class="block h-full bg-track-aqua" style:width="{match.bars[i]}%"></span></dd>{/each}
							</dl>
						</Card>
					{/each}
				</ol>
			</div>
		</div>
	</Section>
</Story>

<!-- Each tool's card with a taste of it working: a drive check and a download queue. The live cards list features instead. Names, kinds and taglines come from #lib/tools. -->
<Story name="Tool previews" asChild>
	<Section labelledby="tools-title">
		<div class="flex flex-col gap-8 sm:gap-12">
			<SectionHeader id="tools-title" eyebrow="Tools for DJs" tone="high" title="From the crate to the booth." />
			<div class="flex flex-wrap gap-4">
				<Card padding="none" radius="2xl" class="flex min-w-0 flex-[1_1_26rem] flex-col overflow-hidden">
					<div class="flex flex-col gap-2 p-5 sm:p-6">
						<h3 class="m-0 flex items-baseline gap-2.5 text-heading font-semibold">{locallibrary.name} <span class="font-mono text-caption font-normal text-fg-subtle">{locallibrary.kind}</span></h3>
						<p class="m-0 text-fg-muted">{locallibrary.tagline}</p>
					</div>
					<div class="mt-auto border-t border-line"><CodeBlock framed={false} language="shell" code={session} /></div>
				</Card>
				<Card padding="none" radius="2xl" class="flex min-w-0 flex-[1_1_26rem] flex-col overflow-hidden">
					<div class="flex flex-col gap-2 p-5 sm:p-6">
						<h3 class="m-0 flex items-baseline gap-2.5 text-heading font-semibold">{localtoolbox.name} <span class="font-mono text-caption font-normal text-fg-subtle">{localtoolbox.kind}</span></h3>
						<p class="m-0 text-fg-muted">{localtoolbox.tagline}</p>
					</div>
					<ol class="m-0 mt-auto list-none border-t border-line bg-canvas px-6 pt-2 pb-4">
						{#each queue as item (item.title)}
							<li class="flex items-center gap-3 border-b border-well py-2 text-small"><span class="min-w-0 flex-auto"><b class="font-semibold">{item.title}</b> <span class="text-fg-muted">{item.artist}</span></span><span class="font-mono text-caption {item.tone}">{item.status}</span></li>
						{/each}
					</ol>
				</Card>
			</div>
		</div>
	</Section>
</Story>
