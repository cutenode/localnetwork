<script module lang="ts">
	import { defineMeta } from '@storybook/addon-svelte-csf';
	import { clock, colors } from '#lib/mix/report.ts';
	import { sampleReport } from '#lib/mix/sample.ts';
	import Badge from './Badge.svelte';
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
	 * The homepage as the design system draws it, section by section. DJs and the mix come from the
	 * seed data and the sample mix report; the events and match scores are examples, since the site
	 * doesn't have events yet.
	 */
	const { Story } = defineMeta({ title: 'Patterns/Homepage', parameters: { layout: 'fullscreen' } });

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
		top: `${(i % 2) * 30}px`
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

	const pillars = [
		['Found by sound', 'A search for house finds deep house and afro house too.'],
		['Matched by distance', 'Results sort by how far each DJ is from your venue.'],
		['Booked in private', 'Every inquiry is its own members-only thread.'],
		['Owned by the DJ', 'Profiles are records on the AT Protocol.']
	];

	const profiles = [
		{ name: 'Lowkey Lagos', kind: 'Collective', city: 'London', tagline: 'Amapiano and afro house crew', genres: 'amapiano · afro house', fee: '£500–2,000' },
		{ name: 'Velvet Static', kind: 'Solo', city: 'Brooklyn', tagline: 'Queer club classics and ballroom heat', genres: 'ballroom · house', fee: '$400–1,200' },
		{ name: 'Footwork Fever', kind: 'Solo', city: 'Chicago', tagline: '160 bpm and climbing', genres: 'footwork · juke', fee: '$200–600' },
		{ name: 'Coyote Cumbia Club', kind: 'Collective', city: 'Mexico City', tagline: 'Cumbia, dembow, and baile funk', genres: 'cumbia · dembow', fee: 'Emerging' }
	];

	const events = [
		{ date: '2026-10-13', title: 'Pirate Signal on air', kind: 'radio', venue: 'Online', time: '20:00 to 22:00', lineup: ['Pirate Signal'], billing: 'Pirate Signal' },
		{ date: '2026-10-16', title: 'Tidal Hours', kind: 'club', venue: 'Halle Nord, Berlin', time: '23:00 till late', lineup: ['Neon Tide'], billing: 'Neon Tide, all night' },
		{ date: '2026-10-17', title: 'Day Shift', kind: 'club', venue: 'Café Zukunft, Berlin', time: '14:00 to 20:00', lineup: ['Saffron Social'], billing: 'Saffron Social' },
		{ date: '2026-10-17', title: 'Lowkey Lagos presents', kind: 'club', venue: 'Arches Yard, London', time: '22:00 to 04:00', lineup: ['Lowkey Lagos', 'Pirate Signal'], billing: 'Lowkey Lagos b2b Pirate Signal' }
	];

	const matches = [
		{ name: 'Saffron Social', why: 'Disco and edits, 3 km away, in budget', bars: [95, 90, 100, 85] },
		{ name: 'Halflight', why: 'Deep house, 27 km away, small rooms', bars: [70, 60, 100, 100] },
		{ name: 'Neon Tide', why: 'House adjacent, 2 km away, top of budget', bars: [50, 100, 45, 90] }
	];

	const factors = ['sound', 'distance', 'budget', 'room'];

	const record = JSON.stringify({ title: 'Tidal Hours', date: '2026-10-16', kind: 'club', venue: 'Halle Nord', location: { locality: 'Berlin', country: 'DE' } }, null, 2);

	const session = ['$ locallibrary check', '✓ USB-A     up to date', '! USB-B     2 playlists behind, syncing', '✓ Ready for Tidal Hours, Fri 16 Oct'].join('\n');

	const queue = [
		{ title: 'Night Bus', artist: 'Ferrule', status: 'free download', tone: 'text-good' },
		{ title: 'Halogen', artist: 'Two Rooms', status: 'gate: follow', tone: 'text-warning' },
		{ title: 'Static Bloom', artist: 'Kestrel Unit', status: 'downloaded', tone: 'text-fg-subtle' },
		{ title: 'Signal Path', artist: 'Mira Okonkwo', status: 'buy link', tone: 'text-link' }
	];
</script>

{#snippet header()}
	<header class="border-b border-line bg-canvas">
		<div class="container-page flex min-h-16 flex-wrap items-center gap-x-8 gap-y-2 py-2">
			<a href="/" class="flex items-center gap-2.5 text-emphasis font-semibold no-underline">
				<span aria-hidden="true" class="flex h-5 items-center gap-0.5">
					{#each [['h-2', 'bg-band-low'], ['h-4.5', 'bg-band-low'], ['h-3', 'bg-band-mid'], ['h-5', 'bg-band-high'], ['h-2.5', 'bg-band-low']] as [h, bg], i (i)}<span class="w-0.75 rounded-[1px] {h} {bg}"></span>{/each}
				</span>
				Local Network
			</a>
			<nav aria-label="Main" class="flex flex-auto flex-wrap gap-x-6.5 gap-y-1 text-small">
				{#each ['DJs', 'Mixes', 'Events', 'For promoters', 'Tools'] as item (item)}<a href="/" class="text-fg-muted no-underline hover:text-fg">{item}</a>{/each}
			</nav>
			<div class="flex items-center gap-2"><Button variant="ghost" size="sm" href="/login">Sign in</Button><Button size="sm" href="/projects/new">List your project</Button></div>
		</div>
	</header>
{/snippet}

{#snippet hero()}
	<section aria-labelledby="hero-title" class="border-b border-line bg-canvas bg-grid">
		<div class="container-page flex flex-col items-center gap-6 pt-16 sm:pt-24">
			<Badge label="New" href="/mixes">Mixes now show every track and transition</Badge>
			<SectionHeader id="hero-title" level={1} size="display" align="center" title="The home for local DJs and the nights they play." lede="Promoters find DJs by sound, distance and budget. DJs keep a profile they own, with their mixes and their nights on it.">
				{#snippet actions()}<Button size="lg" href="/projects">Find DJs near you</Button><Button size="lg" variant="secondary" href="/projects/new">List your project</Button>{/snippet}
			</SectionHeader>
			<div class="mt-8 w-full sm:mt-14">
				<BrowserFrame url="localnetwork.computer/projects/neon-tide" bleed>
					<div class="flex flex-wrap">
						<div class="flex min-w-0 flex-[1_1_20rem] flex-col gap-4 border-line p-7 sm:border-r">
							<div class="flex items-center gap-3.5"><Monogram name="Neon Tide" size="xl" /><div><p class="m-0 text-heading font-semibold">Neon Tide</p><p class="m-0 text-small text-fg-muted">Solo DJ · Berlin, DE · since 2014</p></div></div>
							<p class="m-0 text-fg-soft">Rolling, hypnotic techno for late rooms.</p>
							<div class="flex flex-wrap gap-1.5"><Tag>techno</Tag><Tag>dub techno</Tag><Tag>hypnotic techno</Tag></div>
							<dl class="m-0 grid grid-cols-[auto_1fr] gap-x-5 gap-y-1.5 text-small">
								<dt class="text-fg-subtle">Fee</dt><dd class="m-0 font-mono text-label">€600–1,500</dd>
								<dt class="text-fg-subtle">Rooms</dt><dd class="m-0 font-mono text-label">150–1,500 cap</dd>
								<dt class="text-fg-subtle">Next</dt><dd class="m-0">Tidal Hours, Fri 16 Oct, Halle Nord</dd>
							</dl>
							<div class="mt-auto flex gap-2"><Button size="sm" href="/bookings/new">Send inquiry</Button><Button size="sm" variant="secondary">Save</Button></div>
						</div>
						<div class="flex min-w-0 flex-[2_1_30rem] flex-col gap-3.5 p-7">
							<div class="flex flex-wrap items-baseline justify-between gap-2"><span class="font-semibold">{mix.mix.title}</span><span class="font-mono text-caption text-fg-subtle">{mix.plays.length} tracks · {clock(duration)} · 124–128 BPM</span></div>
							<Waveform bands={mix.waveform} playhead={0.38} label="{mix.mix.title}, {clock(duration)}" class="h-30" />
							{@render lanes()}
						</div>
					</div>
				</BrowserFrame>
			</div>
		</div>
	</section>
{/snippet}

{#snippet lanes()}
	<div class="relative h-14">
		{#each plays as play (play.title)}
			<span class="absolute h-6 truncate rounded-sm px-2 text-caption leading-6 font-semibold" style:left={play.left} style:width={play.width} style:top={play.top} style:background-color={play.swatch.background} style:color={play.swatch.ink}>{play.title}</span>
		{/each}
	</div>
{/snippet}

{#snippet pillarRow()}
	<div class="border-b border-line bg-canvas">
		<ul class="container-page m-0 grid list-none grid-cols-[repeat(auto-fit,minmax(min(100%,15rem),1fr))] p-0">
			{#each pillars as [title, body] (title)}<li class="border-line px-6 py-7 sm:border-r"><p class="m-0 font-semibold">{title}</p><p class="m-0 mt-1 text-small text-fg-muted">{body}</p></li>{/each}
		</ul>
	</div>
{/snippet}

{#snippet profilesSection()}
	<Section labelledby="profiles-title">
		<div class="flex flex-wrap items-center gap-14">
			<div class="flex min-w-0 flex-[1_1_22rem] flex-col gap-6">
				<SectionHeader id="profiles-title" eyebrow="DJ profiles" tone="low" title="A profile built around what gets you booked." lede="Sound, travel radius, room sizes, set lengths and fee range, so promoters can tell in one look if you fit. Solo acts, b2b duos and whole crews, with several managers each." />
				<a href="/projects" class="font-semibold text-link no-underline">Browse the directory →</a>
			</div>
			<ul class="m-0 grid min-w-0 flex-[1_1_30rem] list-none grid-cols-[repeat(auto-fill,minmax(min(100%,15rem),1fr))] gap-3.5 p-0">
				{#each profiles as profile, i (profile.name)}
					<li>
						<Card href="/projects" class="flex h-full flex-col gap-3">
							<span class="flex items-center gap-3"><Monogram name={profile.name} size="lg" /><span><span class="block text-emphasis font-semibold">{profile.name}</span><span class="block text-label text-fg-muted">{profile.kind} · {profile.city}</span></span></span>
							<span class="text-small text-fg-soft">{profile.tagline}</span>
							<Waveform heights={mix.plays[i].track.overview!.heights} colors={mix.plays[i].track.overview!.colors} class="h-7" />
							<span class="flex justify-between gap-2 font-mono text-caption text-fg-muted"><span>{profile.genres}</span><span>{profile.fee}</span></span>
						</Card>
					</li>
				{/each}
			</ul>
		</div>
	</Section>
{/snippet}

{#snippet mixesSection()}
	<Section tone="sunken" labelledby="mixes-title">
		<div class="flex flex-col gap-12">
			<SectionHeader id="mixes-title" align="split" eyebrow="Mixes" tone="mid" title="Every mix, down to the transition." lede="Export a set from locallibrary and the profile shows its waveform, tracklist, tempo, key moves and every blend. Straight from your player history, no typing." />
			<Card padding="none" radius="2xl" class="overflow-hidden">
				<div class="flex flex-wrap items-center justify-between gap-3 border-b border-line px-6 py-4.5">
					<div class="flex items-center gap-3.5">
						<button type="button" aria-label="Play {mix.mix.title}" class="grid size-10 place-items-center rounded-full bg-fg text-fg-inverse"><svg aria-hidden="true" viewBox="0 0 12 14" class="ml-0.5 size-3.5 fill-current"><path d="M0 0l12 7-12 7z" /></svg></button>
						<div><p class="m-0 font-semibold">{mix.mix.title}</p><p class="m-0 text-label text-fg-muted">Saffron Social · Berlin</p></div>
					</div>
					<p class="m-0 flex gap-5 font-mono text-caption text-fg-muted"><span>{clock(duration)}</span><span>124–128 BPM</span><span>8A → 3B</span></p>
				</div>
				<div class="flex flex-col gap-3 p-6">
					<div aria-hidden="true" class="flex h-2.5 gap-0.75">{#each phrases as phrase, i (i)}<span class="rounded-[2px]" style:flex-grow={phrase.grow} style:background-color={phrase.color}></span>{/each}</div>
					<Waveform bands={mix.waveform} label="{mix.mix.title}, {clock(duration)}" class="h-35" />
					{@render lanes()}
				</div>
				<div class="flex flex-wrap border-t border-line">
					<div class="min-w-0 flex-[2_1_30rem] border-line px-6 py-5 sm:border-r">
						<Eyebrow>Tracklist</Eyebrow>
						<ol class="m-0 mt-2.5 list-none p-0">
							{#each plays as play (play.title)}
								<li class="grid grid-cols-[3.25rem_minmax(0,1fr)_3.5rem_2.75rem] items-baseline gap-3 border-t border-well py-2 text-small">
									<span class="font-mono text-caption text-fg-subtle">{play.at}</span>
									<span class="min-w-0"><span class="font-semibold">{play.title}</span> <span class="text-fg-muted">{play.artist}</span></span>
									<span class="text-right font-mono text-caption">{play.bpm}</span>
									<span class="rounded-xs text-center font-mono text-caption" style:background-color={play.swatch.background} style:color={play.swatch.ink}>{play.key}</span>
								</li>
							{/each}
						</ol>
					</div>
					<div class="min-w-0 flex-[1_1_16rem] border-line px-6 py-5 sm:border-r">
						<Eyebrow>Key moves</Eyebrow>
						<div role="img" aria-label="Camelot wheel: 8A, 9A, 10A, then 3B" class="relative mx-auto mt-2.5 aspect-square w-55 max-w-full">
							{#each wheel as key (key.label)}
								<span aria-hidden="true" class={['absolute -mt-3.75 -ml-3.75 grid size-7.5 place-items-center rounded-full font-mono text-[10px]', !key.color && 'bg-well text-fg-subtle']} style:left={key.x} style:top={key.y} style:background-color={key.color?.background} style:color={key.color?.ink}>{key.label}</span>
							{/each}
						</div>
					</div>
					<div class="min-w-0 flex-[1_1_16rem] px-6 py-5">
						<Eyebrow>Tempo</Eyebrow>
						<div aria-hidden="true" class="mt-2.5 flex h-37.5 items-end gap-0.5 border-b border-line-strong">{#each tempo as h, i (i)}<span class="flex-1 border-t-2 border-band-high bg-line-strong" style:height={h}></span>{/each}</div>
						<p class="m-0 mt-1.5 flex justify-between font-mono text-caption text-fg-subtle"><span>124</span><span>126</span><span>128 BPM</span></p>
					</div>
				</div>
			</Card>
		</div>
	</Section>
{/snippet}

{#snippet eventsSection()}
	<Section labelledby="events-title">
		<div class="flex flex-wrap gap-14">
			<div class="flex min-w-0 flex-[1_1_22rem] flex-col gap-6">
				<SectionHeader id="events-title" eyebrow="Events" tone="chorus" title="See who's playing, then book them." lede="Nights near you, from the DJs' own records. After the night, the set lands on the profile with its tracklist." />
				<CodeBlock language="json" code={record} title="computer.localnetwork.dj.set" meta="public record" />
			</div>
			<div class="flex min-w-0 flex-[1_1_34rem] flex-col gap-3">
				<div class="flex flex-wrap items-center justify-between gap-2.5">
					<h3 class="m-0 text-body font-semibold">Next 7 days</h3>
					<div role="group" aria-label="Kind of night" class="flex flex-wrap gap-1.5"><Tag pressed={true}>All</Tag><Tag pressed={false}>Club</Tag><Tag pressed={false}>Radio</Tag></div>
				</div>
				<ol class="m-0 flex list-none flex-col gap-2.5 p-0">
					{#each events as event (event.title)}
						<Card as="li" padding="sm" class="flex flex-wrap items-center gap-x-5 gap-y-3">
							<DateBlock date={event.date} />
							<div class="flex min-w-0 flex-[1_1_14rem] flex-col gap-2">
								<div>
									<p class="m-0 flex flex-wrap items-baseline gap-x-2.5 gap-y-1"><a href="/" class="text-lede font-semibold no-underline">{event.title}</a><Tag variant="mono">{event.kind}</Tag></p>
									<p class="m-0 text-small text-fg-muted">{event.venue} · {event.time}</p>
								</div>
								<p class="m-0 flex items-center gap-2.5 text-small">
									<span class="flex">{#each event.lineup as name (name)}<span class="-mr-2 rounded-[11px] border-2 border-surface"><Monogram {name} size="sm" /></span>{/each}</span>
									<span class="ml-2">{event.billing}</span>
								</p>
							</div>
							<Button size="sm" variant="secondary" href="/bookings/new" class="ml-auto">Book {event.lineup[0]}</Button>
						</Card>
					{/each}
				</ol>
			</div>
		</div>
	</Section>
{/snippet}

{#snippet promotersSection()}
	<Section tone="sunken" labelledby="promoters-title">
		<div class="flex flex-col gap-12">
			<SectionHeader id="promoters-title" eyebrow="For promoters" tone="aqua" title="Describe the night. Get a shortlist with reasons." lede="The matcher scores every DJ on sound, distance, budget and room size, and tells you why each one made the list." />
			<div class="flex flex-wrap gap-4">
				<Card padding="lg" radius="2xl" class="flex min-w-0 flex-[1_1_20rem] flex-col gap-3">
					<Eyebrow>Your show</Eyebrow>
					<dl class="m-0 flex flex-col gap-2">
						{#each [['Date', 'Sat 17 Oct'], ['Venue', 'Kreuzberg, Berlin'], ['Sound', 'disco, house'], ['Room', '300 cap'], ['Budget', '€400–800']] as [term, value] (term)}
							<div class="flex justify-between gap-3 rounded-md bg-well px-3 py-2.5 text-small"><dt class="text-fg-muted">{term}</dt><dd class="m-0 font-medium">{value}</dd></div>
						{/each}
					</dl>
					<Button href="/match">Match DJs</Button>
				</Card>
				<ol class="m-0 flex min-w-0 flex-[2_1_34rem] list-none flex-col gap-3 p-0">
					{#each matches as match (match.name)}
						<Card as="li" padding="md" radius="2xl" class="flex flex-wrap items-center gap-x-7 gap-y-4">
							<div class="flex min-w-0 flex-[1_1_14rem] items-center gap-3"><Monogram name={match.name} /><div><a href="/" class="text-emphasis font-semibold no-underline">{match.name}</a><p class="m-0 text-label text-fg-muted">{match.why}</p></div></div>
							<dl class="m-0 grid min-w-0 flex-[1_1_17rem] grid-cols-[4.5rem_minmax(0,1fr)] items-center gap-x-2.5 gap-y-1.5 font-mono text-caption text-fg-subtle">
								{#each factors as factor, i (factor)}<dt>{factor}</dt><dd class="m-0 h-1.5 overflow-hidden rounded-full bg-line"><span class="block h-full bg-track-aqua" style:width="{match.bars[i]}%"></span></dd>{/each}
							</dl>
						</Card>
					{/each}
				</ol>
			</div>
			<ul class="m-0 grid list-none grid-cols-[repeat(auto-fit,minmax(min(100%,16rem),1fr))] gap-4 p-0">
				{#each [['Private inquiries', 'Each booking is its own thread, shared only with the project’s managers, with your show details up front.'], ['Planner', 'Keep shortlists and build a lineup slot by slot for every show, in a space only you can read.'], ['Booking details', 'Email, phone, rate notes, tech rider and hospitality, visible to every signed-in member.']] as [title, body] (title)}
					<Card as="li" tone="outline" padding="lg" radius="2xl" class="flex flex-col gap-2"><h3 class="m-0 text-emphasis font-semibold">{title}</h3><p class="m-0 text-fg-muted">{body}</p></Card>
				{/each}
			</ul>
		</div>
	</Section>
{/snippet}

{#snippet toolsSection()}
	<Section labelledby="tools-title">
		<div class="flex flex-col gap-12">
			<SectionHeader id="tools-title" eyebrow="Tools for DJs" tone="high" title="From the crate to the booth." />
			<div class="flex flex-wrap gap-4">
				<Card padding="none" radius="2xl" class="flex min-w-0 flex-[1_1_26rem] flex-col overflow-hidden">
					<div class="flex flex-col gap-2 p-6">
						<h3 class="m-0 flex items-baseline gap-2.5 text-heading font-semibold">locallibrary <span class="font-mono text-caption font-normal text-fg-subtle">Command line</span></h3>
						<p class="m-0 text-fg-muted">Keeps rekordbox USB drives in sync, backed up and edited. Checks them before a set, and turns player history into tracklists.</p>
					</div>
					<div class="mt-auto border-t border-line"><CodeBlock framed={false} language="shell" code={session} /></div>
				</Card>
				<Card padding="none" radius="2xl" class="flex min-w-0 flex-[1_1_26rem] flex-col overflow-hidden">
					<div class="flex flex-col gap-2 p-6">
						<h3 class="m-0 flex items-baseline gap-2.5 text-heading font-semibold">localtoolbox <span class="font-mono text-caption font-normal text-fg-subtle">Chrome extension</span></h3>
						<p class="m-0 text-fg-muted">Track lists, download links, artists and a queue, right on SoundCloud. Works through download gates and keeps track of what you've grabbed.</p>
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
{/snippet}

{#snippet closing()}
	<Section tone="grid" labelledby="closing-title">
		<SectionHeader id="closing-title" align="center" title="Your profile, your records, your scene." lede="Everything public is an AT Protocol record you can take anywhere. Booking details stay in private spaces only members can read.">
			{#snippet actions()}<Button size="lg" href="/projects/new">List your project</Button><Button size="lg" variant="secondary" href="/projects">Find DJs near you</Button>{/snippet}
		</SectionHeader>
	</Section>
{/snippet}

{#snippet footer()}
	<footer class="bg-canvas">
		<div class="container-page flex flex-wrap gap-10 py-14">
			<div class="flex flex-[2_1_16rem] flex-col gap-2.5"><span class="font-semibold">Local Network</span><span class="text-small text-fg-subtle">Tools for DJs and the people who book them. Built on the AT Protocol.</span></div>
			{#each [['Product', ['DJs', 'Mixes', 'Events', 'Match a show']], ['Tools', ['locallibrary', 'localtoolbox', 'Docs']], ['About', ['How it works', 'Privacy', 'For DJs']]] as const as [title, links] (title)}
				<nav aria-label={title} class="flex flex-[1_1_9rem] flex-col gap-2 text-small"><span class="font-semibold">{title}</span>{#each links as link (link)}<a href="/" class="text-fg-muted no-underline hover:text-fg">{link}</a>{/each}</nav>
			{/each}
		</div>
	</footer>
{/snippet}

{#snippet page()}
	{@render header()}
	<main>
		{@render hero()}
		{@render pillarRow()}
		{@render profilesSection()}
		{@render mixesSection()}
		{@render eventsSection()}
		{@render promotersSection()}
		{@render toolsSection()}
		{@render closing()}
	</main>
	{@render footer()}
{/snippet}

<Story name="Full page" asChild>{@render page()}</Story>

<Story name="Full page on a phone" asChild globals={{ viewport: { value: 'mobile2', isRotated: false } }}>{@render page()}</Story>

<Story name="Hero" asChild>{@render header()}{@render hero()}</Story>

<Story name="DJ profiles" asChild>{@render profilesSection()}</Story>

<Story name="Mixes" asChild>{@render mixesSection()}</Story>

<Story name="Events" asChild>{@render eventsSection()}</Story>

<Story name="For promoters" asChild>{@render promotersSection()}</Story>

<Story name="Tools" asChild>{@render toolsSection()}</Story>
