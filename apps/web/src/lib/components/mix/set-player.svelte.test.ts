import { page, userEvent } from 'vitest/browser';
import type { z } from 'zod';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from 'vitest-browser-svelte';
import { djSetSchema } from '#lib/dj-schema.ts';
import type { PlayerSource } from '#lib/mix/source.ts';
import { sampleReport } from '#lib/mix/sample.ts';
import DjSets from '../DjSets.svelte';
import MixReport from './MixReport.svelte';
import { silence, sources, stalled, tracks } from './player-fixtures.ts';
import SetPlayer from './SetPlayer.svelte';

const recording = sources(silence(300));

const base = { id: 'player', title: 'Tidal Hours closing set', artist: 'Neon Tide', tracks };

const nowPlaying = (title: string) =>
	page.getByRole('region', { name: /^Player for/ }).getByText(title, { exact: true });

const position = () => page.getByRole('slider', { name: 'Position' });

/** The player starts on its own where the browser allows it (once the page has had a click), so this checks first. */
async function start() {
	const play = page.getByRole('button', { name: 'Play', exact: true });

	if (play.query()) await play.click();
	await expect.element(page.getByRole('button', { name: 'Pause' })).toBeInTheDocument();
}

afterEach(() => {
	vi.unstubAllGlobals();
	vi.restoreAllMocks();
	delete window.Mixcloud;
	delete window.SC;
});

describe('SetPlayer', () => {
	it('shows the recording loading, then plays it when asked', async () => {
		let resolve: (source: PlayerSource) => void = () => {};

		await render(SetPlayer, { ...base, load: () => new Promise<PlayerSource>((done) => (resolve = done)) });
		await expect.element(page.getByRole('status')).toHaveTextContent('Loading the recording');
		await expect.element(page.getByRole('button', { name: 'Play' })).toBeDisabled();

		resolve(recording.pds);
		await expect.element(nowPlaying('Night Bus')).toBeInTheDocument();
		await expect.element(position()).toBeEnabled();
		await start();
		await page.getByRole('button', { name: 'Pause' }).click();
		await expect.element(page.getByRole('button', { name: 'Play' })).toBeEnabled();
	});

	it('follows the tracklist as the recording plays on', async () => {
		const short = [
			{ title: 'First', artist: 'One', startSeconds: 0 },
			{ title: 'Second', artist: 'Two', startSeconds: 1 }
		];

		await render(SetPlayer, { ...base, tracks: short, load: () => Promise.resolve(sources(silence(10)).pds) });
		await expect.element(position()).toBeEnabled();
		await start();
		await expect.element(nowPlaying('First')).toBeInTheDocument();
		// A second of silence plays, then the second track starts.
		await expect.element(nowPlaying('Second'), { timeout: 5_000 }).toBeInTheDocument();
		await expect.element(nowPlaying('Two')).toBeInTheDocument();
	});

	it('seeks with the position bar, and from a track the page picks', async () => {
		const { component } = await render(SetPlayer, { ...base, load: () => Promise.resolve(recording.pds) });
		await expect.element(position()).toBeEnabled();

		await userEvent.fill(position(), '200');
		await expect.element(nowPlaying('Unreleased')).toBeInTheDocument();
		await expect.element(position()).toHaveAttribute('aria-valuetext', '3:20 of 5:00');

		component.seek(260);
		await expect.element(nowPlaying('Halogen')).toBeInTheDocument();
	});

	it('skips between tracks from the keyboard', async () => {
		await render(SetPlayer, { ...base, load: () => Promise.resolve(recording.pds) });
		await expect.element(position()).toBeEnabled();

		await start();
		await userEvent.tab();
		await expect.element(page.getByRole('button', { name: 'Pause' })).toHaveFocus();
		await userEvent.keyboard('{Enter}');
		await expect.element(page.getByRole('button', { name: 'Play' })).toHaveFocus();
		await userEvent.keyboard(' ');
		await expect.element(page.getByRole('button', { name: 'Pause' })).toHaveFocus();

		await userEvent.tab();
		await userEvent.tab();
		await expect.element(page.getByRole('button', { name: 'Next track' })).toHaveFocus();
		await userEvent.keyboard('{Enter}');
		await expect.element(nowPlaying('Low Tide (Dub)')).toBeInTheDocument();
		await userEvent.keyboard('{Enter}');
		await expect.element(nowPlaying('Unreleased')).toBeInTheDocument();

		await userEvent.tab({ shift: true });
		await userEvent.keyboard('{Enter}');
		await expect.element(nowPlaying('Low Tide (Dub)')).toBeInTheDocument();

		await userEvent.tab();
		await userEvent.tab();
		await expect.element(position()).toHaveFocus();
		await userEvent.keyboard('{End}');
		await expect.element(nowPlaying('Halogen')).toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Next track' })).toBeDisabled();
	});

	it("names the track playing in the system's media controls", async () => {
		await render(SetPlayer, {
			...base,
			load: () => Promise.resolve({ ...recording.plyr, artwork: 'https://images.test/cover.jpg' })
		});
		await userEvent.fill(position(), '100');
		await expect.element(nowPlaying('Low Tide (Dub)')).toBeInTheDocument();

		expect(navigator.mediaSession.metadata).toMatchObject({
			title: 'Low Tide (Dub)',
			artist: 'Ferrule, Ava Lind',
			album: base.title
		});
		expect(navigator.mediaSession.metadata?.artwork.map((art) => art.src)).toEqual(['https://images.test/cover.jpg']);
	});

	it("says when the recording won't play, and tries again", async () => {
		let attempts = 0;

		const load = () => (++attempts === 1 ? Promise.reject(new Error('HTTP 502')) : Promise.resolve(recording.pds));

		await render(SetPlayer, { ...base, load });
		await expect.element(page.getByText("This recording won't play right now.")).toBeInTheDocument();
		await page.getByRole('button', { name: 'Try again' }).click();
		await expect.element(nowPlaying('Night Bus')).toBeInTheDocument();
		await expect.element(page.getByText("This recording won't play right now.")).not.toBeInTheDocument();
	});

	it('offers the recording elsewhere when its audio fails', async () => {
		await render(SetPlayer, {
			...base,
			load: () => Promise.resolve({ ...recording.plyr, src: 'data:audio/wav;base64,AAAA' })
		});
		await expect
			.element(page.getByRole('link', { name: 'Listen on plyr.fm' }))
			.toHaveAttribute('href', 'https://plyr.fm/track/1567');
	});

	it('plays from a track picked before the recording loaded', async () => {
		let resolve: (source: PlayerSource) => void = () => {};

		const { component } = await render(SetPlayer, {
			...base,
			load: () => new Promise<PlayerSource>((done) => (resolve = done))
		});

		component.seek(200);
		resolve(recording.pds);

		await expect.element(nowPlaying('Unreleased')).toBeInTheDocument();
		await expect.element(position()).toHaveAttribute('aria-valuetext', '3:20 of 5:00');
	});

	it('says when the server finds nothing to play, with nowhere else to listen', async () => {
		await render(SetPlayer, { ...base, load: () => Promise.resolve(null) });

		await expect.element(page.getByText("This recording won't play right now.")).toBeInTheDocument();
		await expect.element(page.getByRole('link')).not.toBeInTheDocument();
	});

	it('ignores a recording that arrives after the player closed', async () => {
		let resolve: (source: PlayerSource) => void = () => {};

		let reject: (error: Error) => void = () => {};

		const first = await render(SetPlayer, {
			...base,
			load: () => new Promise<PlayerSource>((done) => (resolve = done))
		});

		const second = await render(SetPlayer, {
			...base,
			id: 'other',
			load: () => new Promise<PlayerSource>((_, fail) => (reject = fail))
		});

		await first.unmount();
		await second.unmount();
		resolve(recording.pds);
		reject(new Error('HTTP 502'));

		await expect.element(page.getByRole('region', { name: /^Player for/ })).not.toBeInTheDocument();
	});

	it('names the recording until its first track starts, and a track by the set when it has no artist of its own', async () => {
		const later = [{ title: 'Intro', startSeconds: 30 }, ...tracks.slice(1)];

		await render(SetPlayer, { ...base, tracks: later, load: () => Promise.resolve(recording.pds) });
		await expect.element(position()).toBeEnabled();
		await expect.element(nowPlaying(base.title)).toBeInTheDocument();
		await expect.element(nowPlaying('Neon Tide')).toBeInTheDocument();
		expect(navigator.mediaSession.metadata).toMatchObject({ title: base.title, artist: 'Neon Tide' });

		await userEvent.fill(position(), '200');
		await expect.element(nowPlaying('Unreleased')).toBeInTheDocument();
		expect(navigator.mediaSession.metadata).toMatchObject({ title: 'Unreleased', artist: 'Neon Tide' });
		await expect.element(nowPlaying('Neon Tide')).not.toBeInTheDocument();
	});

	it('leaves the artist blank where neither the track nor the set names one', async () => {
		const later = [{ title: 'Intro', startSeconds: 30 }];

		await render(SetPlayer, { ...base, artist: undefined, tracks: later, load: () => Promise.resolve(recording.pds) });
		await expect.element(position()).toBeEnabled();
		expect(navigator.mediaSession.metadata).toMatchObject({ title: base.title, artist: '' });

		await userEvent.fill(position(), '40');
		await expect.element(nowPlaying('Intro')).toBeInTheDocument();
		expect(navigator.mediaSession.metadata).toMatchObject({ title: 'Intro', artist: '' });
	});

	it("is steered by the system's media controls", async () => {
		const handlers = new Map<MediaSessionAction, MediaSessionActionHandler | null>();
		vi.spyOn(navigator.mediaSession, 'setActionHandler').mockImplementation((action, handler) =>
			handlers.set(action, handler)
		);

		const press = (action: MediaSessionAction, details: Partial<MediaSessionActionDetails> = {}) =>
			handlers.get(action)?.({ action, ...details });

		await render(SetPlayer, { ...base, load: () => Promise.resolve(recording.pds) });
		await expect.element(position()).toBeEnabled();
		press('pause');
		await expect.element(page.getByRole('button', { name: 'Play', exact: true })).toBeInTheDocument();

		press('seekto', { seekTime: 200 });
		await expect.element(position()).toHaveAttribute('aria-valuetext', '3:20 of 5:00');
		press('seekto');
		press('seekforward');
		await expect.element(position()).toHaveAttribute('aria-valuetext', '3:30 of 5:00');
		press('seekbackward', { seekOffset: 100 });
		await expect.element(nowPlaying('Low Tide (Dub)')).toBeInTheDocument();
		press('seekbackward');
		await expect.element(position()).toHaveAttribute('aria-valuetext', '1:40 of 5:00');
		press('seekforward', { seekOffset: 15 });
		await expect.element(position()).toHaveAttribute('aria-valuetext', '1:55 of 5:00');

		// Past the last track's start there's no next one to skip to.
		press('seekto', { seekTime: 290 });
		press('nexttrack');
		await expect.element(position()).toHaveAttribute('aria-valuetext', '4:50 of 5:00');

		press('play');
		await expect.element(page.getByRole('button', { name: 'Pause' })).toBeInTheDocument();
		press('pause');
		await expect.element(page.getByRole('button', { name: 'Play', exact: true })).toBeInTheDocument();

		press('seekto', { seekTime: 115 });
		press('nexttrack');
		await expect.element(nowPlaying('Unreleased')).toBeInTheDocument();
		press('previoustrack');
		await expect.element(nowPlaying('Low Tide (Dub)')).toBeInTheDocument();
	});

	it('goes back to the start from before the first track', async () => {
		const later = [{ title: 'Intro', startSeconds: 30 }, ...tracks.slice(1)];

		await render(SetPlayer, { ...base, tracks: later, load: () => Promise.resolve(recording.pds) });
		await userEvent.fill(position(), '20');
		await vi.waitFor(() => expect(position().element().getAttribute('aria-valuetext')).toMatch(/^0:2\d of 5:00$/));

		await page.getByRole('button', { name: 'Previous track' }).click();

		await vi.waitFor(() => expect(position().element().getAttribute('aria-valuetext')).toMatch(/^0:0\d of 5:00$/));
	});

	it('lets go of a play the recording never started', async () => {
		const play = page.getByRole('button', { name: 'Play', exact: true });
		const pause = page.getByRole('button', { name: 'Pause' });
		let resolve: (source: PlayerSource) => void = () => {};

		const { component } = await render(SetPlayer, {
			...base,
			load: () => new Promise<PlayerSource>((done) => (resolve = done))
		});

		// A click on the page first, so the browser lets the player start on its own.
		await page.getByText('0:00').click();
		resolve({ kind: 'audio', src: stalled(), from: 'pds' });

		await expect.element(pause).toBeEnabled();
		await expect.element(position()).toBeDisabled();
		await expect.element(page.getByText('-:--')).toBeInTheDocument();
		await pause.click();
		await play.click();
		await pause.click();
		await expect.element(play).toBeEnabled();
		component.seek(100);
		await pause.click();

		await expect.element(play).toBeEnabled();
		await expect.element(position()).toHaveAttribute('aria-valuetext', '1:40 of 0:00');
	});

	it("plays a platform's player, following its tracklist where the widget API allows", async () => {
		let progress: ((position: number, duration: number) => void) | undefined;
		const events = { on: () => {}, off: () => {} };

		window.Mixcloud = {
			PlayerWidget: () => ({
				ready: Promise.resolve(),
				events: {
					progress: { on: (handler) => (progress = handler), off: () => {} },
					play: events,
					pause: events,
					ended: events
				},
				play: () => Promise.resolve(),
				// The widget reports where it got to, as Mixcloud's does once it has seeked.
				seek: async (seconds) => {
					progress?.(seconds, 300);

					return true;
				}
			})
		};

		const embed: PlayerSource = {
			kind: 'embed',
			src: 'about:blank',
			platform: 'Mixcloud',
			link: 'https://www.mixcloud.com/neon/tidal/'
		};

		const { component } = await render(SetPlayer, { ...base, load: () => Promise.resolve(embed) });
		await expect.element(page.getByTitle(base.title)).toBeInTheDocument();

		await vi.waitFor(() => expect(progress).toBeTypeOf('function'));
		progress?.(100, 300);
		await expect.element(page.getByText('Low Tide (Dub)')).toBeInTheDocument();
		component.seek(250);
		await expect.element(page.getByText('Halogen')).toBeInTheDocument();
	});

	it('says nothing is playing on a platform player until the first track starts, and leaves out an artist it lacks', async () => {
		const mixcloud = fakeMixcloud();
		window.Mixcloud = { PlayerWidget: () => mixcloud.widget };
		const later = [{ title: 'Intro', startSeconds: 30 }, ...tracks.slice(1)];

		await render(SetPlayer, { ...base, tracks: later, load: () => Promise.resolve(mixcloudEmbed) });
		await vi.waitFor(() => expect(mixcloud.listening()).toBe(true));
		await expect.element(page.getByText(/Now playing/)).not.toBeInTheDocument();

		mixcloud.progress(200);
		await expect.element(page.getByText('Now playing Unreleased', { exact: true })).toBeInTheDocument();
	});

	it('stops following a platform player that closed before its widget answered', async () => {
		let ready: () => void = () => {};

		const mixcloud = fakeMixcloud(new Promise<void>((done) => (ready = done)));
		window.Mixcloud = { PlayerWidget: () => mixcloud.widget };

		const { unmount } = await render(SetPlayer, { ...base, load: () => Promise.resolve(mixcloudEmbed) });
		await expect.element(page.getByTitle(base.title)).toBeInTheDocument();
		await unmount();
		ready();

		await vi.waitFor(() => expect(mixcloud.closed()).toBe(true));
		expect(mixcloud.listening()).toBe(false);
	});

	it("follows SoundCloud's player the same way", async () => {
		const handlers = new Map<string, (event?: { currentPosition: number }) => void>();
		const Events = { READY: 'ready', PLAY: 'play', PAUSE: 'pause', FINISH: 'finish', PLAY_PROGRESS: 'playProgress' };

		const Widget = Object.assign(
			() => ({
				bind: (event: string, handler: (event?: { currentPosition: number }) => void) => {
					handlers.set(event, handler);

					// The player is ready as soon as the page asks.
					if (event === Events.READY) handler();
				},
				unbind: (event: string) => handlers.delete(event),
				play: () => handlers.get(Events.PLAY)?.(),
				// Like SoundCloud's, it reports the new position in milliseconds.
				seekTo: (milliseconds: number) => handlers.get(Events.PLAY_PROGRESS)?.({ currentPosition: milliseconds })
			}),
			{ Events }
		);

		window.SC = { Widget };

		const embed: PlayerSource = {
			kind: 'embed',
			src: 'about:blank',
			platform: 'SoundCloud',
			link: 'https://soundcloud.com/neon/tidal'
		};

		const { component } = await render(SetPlayer, { ...base, load: () => Promise.resolve(embed) });

		await vi.waitFor(() => expect(handlers.has(Events.PLAY_PROGRESS)).toBe(true));
		handlers.get(Events.PLAY_PROGRESS)?.({ currentPosition: 100_000 });
		await expect.element(page.getByText('Low Tide (Dub)')).toBeInTheDocument();
		component.seek(250);
		await expect.element(page.getByText('Halogen')).toBeInTheDocument();
	});

	it('still plays a platform player whose widget API fails', async () => {
		window.Mixcloud = {
			PlayerWidget: () => {
				throw new Error('no widget');
			}
		};

		const embed: PlayerSource = {
			kind: 'embed',
			src: 'about:blank',
			platform: 'Mixcloud',
			link: 'https://www.mixcloud.com/neon/tidal/'
		};

		await render(SetPlayer, { ...base, load: () => Promise.resolve(embed) });
		await expect.element(page.getByTitle(base.title)).toBeInTheDocument();
		await expect.element(page.getByText(/Now playing/)).not.toBeInTheDocument();
	});

	it("leaves a player it can't follow to play on its own", async () => {
		await render(SetPlayer, { ...base, load: () => Promise.resolve(recording.embed) });
		await expect.element(page.getByTitle(base.title)).toBeInTheDocument();
		await expect.element(page.getByText(/Now playing/)).not.toBeInTheDocument();
		await expect.element(position()).not.toBeInTheDocument();
	});
});

describe('DjSets with a recording', () => {
	const set = (rkey: string, value: z.input<typeof djSetSchema>) => ({
		...djSetSchema.parse(value),
		uri: `at://did:plc:neon/computer.localnetwork.dj.set/${rkey}`
	});

	it('asks the server where it plays, then marks and starts tracks from the tracklist', async () => {
		const asked: string[] = [];
		const audio = sources(silence(300)).pds;

		vi.stubGlobal('fetch', async (input: string) => {
			asked.push(input);

			return Response.json({ source: audio });
		});

		await render(DjSets, {
			sets: [
				set('3kset', {
					date: '2026-09-26',
					venue: 'Halle Nord',
					links: [{ uri: 'https://archive.org/download/tidal/tidal.mp3' }],
					tracks: tracks.map(({ title, artist, startSeconds }) => ({ title, artist, startSeconds }))
				}),
				set('3kother', { date: '2026-09-12', venue: 'Tresor', tracks: [] })
			]
		});
		await expect.element(page.getByRole('button', { name: 'Play Tresor' })).not.toBeInTheDocument();

		await page.getByRole('button', { name: 'Play Halle Nord' }).click();
		expect(asked).toEqual(['/sets/did:plc:neon/3kset/player.json']);
		await expect.element(position()).toBeEnabled();

		await page.getByRole('button', { name: 'Play from 4:10: Halogen' }).click();
		// The set's own list item holds every track too, so each track's row is the last match.
		const row = (title: string) => page.getByRole('listitem').filter({ hasText: title }).last();
		await expect.element(row('Halogen')).toHaveAttribute('aria-current', 'true');

		await row('Low Tide (Dub)').getByText('Low Tide (Dub)', { exact: true }).click();
		await expect.element(row('Low Tide (Dub)')).toHaveAttribute('aria-current', 'true');
		await expect.element(row('Halogen')).not.toHaveAttribute('aria-current');

		await page.getByRole('button', { name: 'Close the player for Halle Nord' }).click();
		await expect.element(position()).not.toBeInTheDocument();
		// The tracklist opened with the player and folds away with it.
		await expect.element(page.getByRole('group')).not.toHaveAttribute('open');
	});
});

describe('MixReport with a recording', () => {
	it('plays from the track picked, and marks the track playing', async () => {
		await render(MixReport, { report: sampleReport, source: Promise.resolve(sources(silence(700)).pds) });
		await page.getByRole('button', { name: 'Play', exact: true }).click();
		await expect.element(position()).toBeEnabled();

		await page.getByRole('button', { name: '3 Halogen Two Rooms', exact: true }).click();
		await expect
			.element(page.getByRole('button', { name: 'Playing now: 3 Halogen Two Rooms', exact: true }))
			.toBeInTheDocument();
		await expect.element(position()).toHaveAttribute('aria-valuetext', '10:10 of 11:40');
	});

	it('stops marking the track playing once the player closes', async () => {
		await render(MixReport, { report: sampleReport, source: Promise.resolve(sources(silence(700)).pds) });
		await page.getByRole('button', { name: 'Play', exact: true }).click();
		await expect.element(page.getByRole('button', { name: /^Playing now: 1\. Night Bus/ })).toBeInTheDocument();

		await page.getByRole('button', { name: 'Close player' }).click();

		await expect.element(position()).not.toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: /^Playing now:/ })).not.toBeInTheDocument();
	});

	it("credits the set's artist for a track that has none of its own", async () => {
		const [first, ...rest] = sampleReport.plays;
		const report = { ...sampleReport, plays: [{ ...first, track: { ...first.track, artist: undefined } }, ...rest] };

		await render(MixReport, { report, source: Promise.resolve(sources(silence(700)).pds) });
		await page.getByRole('button', { name: 'Play', exact: true }).click();
		await expect.element(page.getByRole('button', { name: /^Playing now: 1\. Night Bus/ })).toBeInTheDocument();

		expect(navigator.mediaSession.metadata).toMatchObject({ title: 'Night Bus', artist: 'Sample DJ' });
	});

	it('has no Play button when nothing plays', async () => {
		await render(MixReport, { report: sampleReport, listen: 'https://example.com/mix', source: Promise.resolve(null) });
		await expect.element(page.getByRole('link', { name: /Listen on example.com/ })).toBeInTheDocument();
		await expect.element(page.getByRole('button', { name: 'Play', exact: true })).not.toBeInTheDocument();
	});
});

// Helpers

const mixcloudEmbed: PlayerSource = {
	kind: 'embed',
	src: 'about:blank',
	platform: 'Mixcloud',
	link: 'https://www.mixcloud.com/neon/tidal/'
};

/** A Mixcloud player widget, ready once `ready` resolves, that says whether the page listens to it. */
function fakeMixcloud(ready: Promise<void> = Promise.resolve()) {
	const progress = new Set<(position: number, duration: number) => void>();
	let followed = false;
	let destroyed = false;
	const events = { on: () => {}, off: () => {} };

	const widget = {
		ready: ready.then(() => (followed = true)),
		events: {
			progress: {
				on: (handler: (position: number, duration: number) => void) => progress.add(handler),
				off: (handler: (position: number, duration: number) => void) => {
					progress.delete(handler);
					destroyed = true;
				}
			},
			play: events,
			pause: events,
			ended: events
		},
		play: () => Promise.resolve(),
		seek: () => Promise.resolve(true)
	};

	return {
		widget,
		listening: () => followed && progress.size > 0,
		closed: () => destroyed,
		progress: (seconds: number) => progress.forEach((handler) => handler(seconds, 300))
	};
}
