<script lang="ts">
	import { ExternalLink, Pause, Play, SkipBack, SkipForward } from '@lucide/svelte';
	import { untrack } from 'svelte';
	import { mixPlatform } from '#lib/mix/links.ts';
	import { clock } from '#lib/mix/report.ts';
	import { type PlayerSource, type PlayerTrack, trackAt } from '#lib/mix/source.ts';
	import { follow, type Widget } from '#lib/mix/widgets.ts';
	import Button from '#lib/ui/Button.svelte';
	import MixEmbed from './MixEmbed.svelte';

	/**
	 * A set's recording, following its tracklist. `load` asks where it plays (the server decides, by preference: see
	 * `#lib/mix/source.ts`), again when someone tries again. `title` and `artist` name the recording in the system's
	 * media controls. `current` is the track playing now, by position in `tracks`, for the page's tracklist to mark;
	 * `seek` plays from a point, for its tracks to start.
	 */
	let {
		id,
		load,
		title,
		artist,
		tracks,
		current = $bindable(-1)
	}: { id: string; load: () => Promise<PlayerSource | null>; title: string; artist?: string; tracks: PlayerTrack[]; current?: number } = $props();

	let status = $state<'loading' | 'ready' | 'failed'>('loading');

	let source = $state.raw<PlayerSource | null>(null);

	let attempt = $state(0);

	let audio = $state<HTMLAudioElement>();

	let iframe = $state<HTMLIFrameElement>();

	/** The platform player the page follows, when its widget API allows. */
	let widget = $state.raw<Widget | null>(null);

	let time = $state(0);

	let duration = $state(0);

	let paused = $state(true);

	/** Where each track starts, in order, for skipping between them and marking them on the bar. */
	const marks = $derived([...new Set(tracks.flatMap((track) => (track.startSeconds === undefined ? [] : [track.startSeconds])))].sort((a, b) => a - b));

	const playing = $derived(tracks[current]);

	const hasNext = $derived(marks.some((mark) => mark > time + 0.5));

	$effect(() => {
		current = trackAt(tracks, time);
	});

	$effect(() => {
		void attempt;
		let stale = false;
		status = 'loading';
		source = null;
		void (async () => {
			try {
				// `load` is often a new function on every render of the page; only a retry should ask again.
				const found = await untrack(load);

				if (stale) return;
				source = found;
				status = found ? 'ready' : 'failed';
			} catch {
				if (!stale) status = 'failed';
			}
		})();

		return () => {
			stale = true;
		};
	});

	// Someone pressed Play to open the player, so it starts. A browser that won't allow it leaves it paused.
	$effect(() => {
		audio?.play().catch(() => undefined);
	});

	$effect(() => {
		if (!iframe || source?.kind !== 'embed') return;
		let active = true;
		let followed: Widget | null = null;

		const { platform } = source;
		const element = iframe;

		void (async () => {
			try {
				const found = await follow(platform, element, { time: (seconds) => (time = seconds), playing: (on) => (paused = !on) });

				if (!active) return found?.destroy();
				followed = widget = found;
			} catch {
				// The platform's player still plays on its own; only following it is lost.
			}
		})();

		return () => {
			active = false;
			followed?.destroy();
			widget = null;
		};
	});

	/** Moves to `seconds` into the recording without changing whether it plays. */
	function jump(seconds: number) {
		time = Math.max(0, seconds);

		if (audio) audio.currentTime = time;
	}

	/** Plays from `seconds` into the recording. */
	export function seek(seconds: number) {
		if (widget) return widget.seek(seconds);
		jump(seconds);
		audio?.play().catch(() => undefined);
	}

	/** Back to the start of this track, or to the one before when it only just started. */
	const previous = () => seek(marks.findLast((mark) => mark <= time - 3) ?? 0);

	const next = () => {
		const mark = marks.find((start) => start > time + 0.5);

		if (mark !== undefined) seek(mark);
	};

	const toggle = () => (paused ? audio?.play().catch(() => undefined) : audio?.pause());

	// The system's media controls (lock screen, headphones, keyboard media keys) show and steer the audio.
	$effect(() => {
		if (!audio || !('mediaSession' in navigator)) return;
		const artwork = source?.kind === 'audio' && source.artwork ? [{ src: source.artwork }] : [];

		navigator.mediaSession.metadata = new MediaMetadata(
			playing ? { title: playing.title, artist: playing.artist ?? artist ?? '', album: title, artwork } : { title, artist: artist ?? '', artwork }
		);
	});

	$effect(() => {
		if (!audio || !('mediaSession' in navigator)) return;
		const media = audio;
		const session = navigator.mediaSession;

		const actions: [MediaSessionAction, MediaSessionActionHandler][] = [
			['play', () => void media.play().catch(() => undefined)],
			['pause', () => media.pause()],
			['seekto', (details) => jump(details.seekTime ?? media.currentTime)],
			['seekbackward', (details) => jump(media.currentTime - (details.seekOffset ?? 10))],
			['seekforward', (details) => jump(media.currentTime + (details.seekOffset ?? 10))],
			['previoustrack', previous],
			['nexttrack', next]
		];

		const set = (handler: (action: MediaSessionAction, run: MediaSessionActionHandler) => void) => {
			for (const [action, run] of actions) {
				try {
					handler(action, run);
				} catch {
					// Browsers throw for actions they don't know, which the rest can do without.
				}
			}
		};

		set((action, run) => session.setActionHandler(action, run));

		return () => {
			set((action) => session.setActionHandler(action, null));
			session.metadata = null;
			session.playbackState = 'none';
		};
	});

	$effect(() => {
		if (!audio || !('mediaSession' in navigator)) return;
		navigator.mediaSession.playbackState = paused ? 'paused' : 'playing';

		if (duration > 0) navigator.mediaSession.setPositionState({ duration, position: Math.min(time, duration), playbackRate: 1 });
	});

	const heading = $derived(playing?.title ?? title);

	const byline = $derived(playing ? (playing.artist ?? '') : (artist ?? ''));

	const elsewhere = $derived(source?.link ? (source.kind === 'embed' ? source.platform : mixPlatform(source.link)) : null);
</script>

<section {id} aria-label="Player for {title}" class="flex flex-col gap-2 print:hidden">
	{#if status === 'failed'}
		<div class="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 rounded-lg border border-line bg-well px-3 py-2.5">
			<p class="m-0 text-small text-fg-muted">This recording won't play right now.</p>
			<div class="flex items-center gap-4">
				{#if source?.link}<a class="link inline-flex items-center gap-1 text-small" href={source.link} rel="noopener noreferrer nofollow" target="_blank">Listen on {elsewhere}<ExternalLink class="size-3" /></a>{/if}
				<Button variant="secondary" size="sm" onclick={() => attempt++}>Try again</Button>
			</div>
		</div>
	{:else if source?.kind === 'embed'}
		<MixEmbed id="{id}-embed" src={source.src} {title} bind:element={iframe} />
		{#if widget}
			<p class="m-0 min-h-5 truncate text-small text-fg-muted" aria-live="polite">
				{#if playing}Now playing <span class="font-semibold text-fg">{playing.title}</span>{#if playing.artist}{' '}· {playing.artist}{/if}{/if}
			</p>
		{/if}
	{:else}
		{@const ready = status === 'ready'}
		<div class="flex flex-col gap-2 rounded-lg border border-line bg-well p-3">
			<div class="flex items-center gap-3">
				<button
					type="button"
					aria-label={paused ? 'Play' : 'Pause'}
					disabled={!ready}
					class="grid size-10 shrink-0 cursor-pointer place-items-center rounded-full bg-fg text-fg-inverse transition-opacity hover:opacity-85 disabled:cursor-default disabled:opacity-50"
					onclick={toggle}
				>
					{#if paused}<Play class="ml-0.5 size-4 fill-current" />{:else}<Pause class="size-4 fill-current" />{/if}
				</button>
				<div class="min-w-0 flex-1">
					{#if ready}
						<p class="m-0 truncate text-small font-semibold" aria-live="polite">{heading}</p>
						<p class="m-0 min-h-[1.05rem] truncate text-caption text-fg-muted">{byline}</p>
					{:else}
						<span aria-hidden="true" class="block h-3.5 w-48 max-w-full rounded-sm bg-raised motion-safe:animate-pulse"></span>
						<span aria-hidden="true" class="mt-1.5 block h-3 w-28 max-w-full rounded-sm bg-raised motion-safe:animate-pulse"></span>
						<span class="sr-only" role="status">Loading the recording</span>
					{/if}
				</div>
				<button type="button" aria-label="Previous track" disabled={!ready || !marks.length} class="btn-ghost btn-sm size-9 px-0" onclick={previous}><SkipBack class="size-4" /></button>
				<button type="button" aria-label="Next track" disabled={!ready || !hasNext} class="btn-ghost btn-sm size-9 px-0" onclick={next}><SkipForward class="size-4" /></button>
			</div>
			<div class="flex items-center gap-3 font-mono text-caption text-fg-subtle tabular-nums">
				<span class="min-w-10">{clock(time)}</span>
				<div class="relative flex flex-1 items-center">
					{#if duration > 0}
						{#each marks as mark (mark)}
							<span aria-hidden="true" class="pointer-events-none absolute top-1/2 h-3 w-px -translate-y-1/2 bg-fg-subtle" style:left="{(mark / duration) * 100}%"></span>
						{/each}
					{/if}
					<input
						type="range"
						aria-label="Position"
						aria-valuetext="{clock(time)} of {clock(duration)}"
						min="0"
						max={duration || 0}
						step="1"
						value={time}
						disabled={!ready || !duration}
						class="relative w-full cursor-pointer accent-fg disabled:cursor-default"
						oninput={(event) => jump(Number(event.currentTarget.value))}
					/>
				</div>
				<span class="min-w-10 text-right">{duration ? clock(duration) : '-:--'}</span>
			</div>
		</div>
		{#if source?.kind === 'audio'}
			<audio
				bind:this={audio}
				src={source.src}
				preload="metadata"
				ontimeupdate={() => (time = audio?.currentTime ?? time)}
				onloadedmetadata={() => {
					duration = Number.isFinite(audio?.duration) ? (audio?.duration ?? 0) : 0;
					// A track picked before the recording loaded.
					if (audio && time) audio.currentTime = time;
				}}
				onplay={() => (paused = false)}
				onpause={() => (paused = true)}
				onerror={() => (status = 'failed')}
			></audio>
		{/if}
	{/if}
</section>
