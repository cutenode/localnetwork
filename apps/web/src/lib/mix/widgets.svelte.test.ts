import { afterEach, describe, expect, it, vi } from 'vitest';
import { follow, type WidgetEvents } from './widgets.ts';

const MIXCLOUD_API = 'https://widget.mixcloud.com/media/js/widgetApi.js';

const SOUNDCLOUD_API = 'https://w.soundcloud.com/player/api.js';

afterEach(() => {
	vi.restoreAllMocks();
	delete window.Mixcloud;
	delete window.SC;
});

describe('follow', () => {
	it('resolves null for a platform whose player it cannot follow', async () => {
		// Arrange
		const { on } = listener();

		// Act
		const widget = await follow('YouTube', iframe(), on);

		// Assert
		expect(widget).toBeNull();
	});

	it("loads Mixcloud's widget API once, then follows and seeks its player", async () => {
		// Arrange
		const mixcloud = fakeMixcloud();
		const scripts = serveScripts(() => (window.Mixcloud = { PlayerWidget: () => mixcloud.widget }));
		const { on, heard } = listener();

		// Act
		const [widget, again] = await Promise.all([
			follow('Mixcloud', iframe(), on),
			follow('Mixcloud', iframe(), listener().on)
		]);

		mixcloud.emit('progress', 95, 300);
		mixcloud.emit('play');
		mixcloud.emit('pause');
		mixcloud.emit('play');
		mixcloud.emit('ended');
		widget?.seek(250);
		await vi.waitFor(() => expect(mixcloud.seeked).toEqual([250]));

		// Assert
		expect(scripts.map((script) => [script.src, script.async])).toEqual([[MIXCLOUD_API, true]]);
		expect(again).not.toBeNull();
		expect(heard).toEqual([
			['time', 95],
			['playing', true],
			['playing', false],
			['playing', true],
			['playing', false]
		]);
		expect(mixcloud.played).toBe(1);
	});

	it('stops listening to a Mixcloud player once destroyed', async () => {
		// Arrange
		const mixcloud = fakeMixcloud();
		window.Mixcloud = { PlayerWidget: () => mixcloud.widget };
		const { on, heard } = listener();
		const widget = await follow('Mixcloud', iframe(), on);

		// Act
		widget?.destroy();
		mixcloud.emit('progress', 95, 300);
		mixcloud.emit('play');
		mixcloud.emit('pause');
		mixcloud.emit('ended');

		// Assert
		expect(heard).toEqual([]);
	});

	it("says when SoundCloud's widget API won't load, and loads it the next time a player opens", async () => {
		// Arrange
		serveScripts(() => {}, 'error');
		const { on } = listener();

		// Act
		const failed = follow('SoundCloud', iframe(), on);

		// Assert
		await expect(failed).rejects.toThrow(`could not load ${SOUNDCLOUD_API}`);
		vi.restoreAllMocks();
		const retried = serveScripts(() => {});
		await expect(follow('SoundCloud', iframe(), on)).rejects.toThrow(`${SOUNDCLOUD_API} has no API`);
		expect(retried.map((script) => script.src)).toEqual([SOUNDCLOUD_API]);
	});

	it("follows and seeks SoundCloud's player, in its milliseconds", async () => {
		// Arrange
		const soundcloud = fakeSoundCloud();
		window.SC = { Widget: soundcloud.Widget };
		const { on, heard } = listener();
		const widget = await follow('SoundCloud', iframe(), on);

		// Act
		soundcloud.emit('playProgress', { currentPosition: 95_000 });
		soundcloud.emit('playProgress');
		soundcloud.emit('play');
		soundcloud.emit('pause');
		soundcloud.emit('finish');
		widget?.seek(250);

		// Assert
		expect(heard).toEqual([
			['time', 95],
			['time', 0],
			['playing', true],
			['playing', false],
			['playing', false],
			['time', 250],
			['playing', true]
		]);
	});

	it('stops listening to a SoundCloud player once destroyed', async () => {
		// Arrange
		const soundcloud = fakeSoundCloud();
		window.SC = { Widget: soundcloud.Widget };
		const { on, heard } = listener();
		const widget = await follow('SoundCloud', iframe(), on);

		// Act
		widget?.destroy();
		soundcloud.emit('playProgress', { currentPosition: 95_000 });
		soundcloud.emit('play');

		// Assert
		expect(heard).toEqual([]);
	});
});

// Helpers

const iframe = () => document.createElement('iframe');

/** What the page heard from a player, in order. */
function listener() {
	const heard: [string, number | boolean][] = [];

	const on: WidgetEvents = {
		time: (seconds) => heard.push(['time', seconds]),
		playing: (playing) => heard.push(['playing', playing])
	};

	return { on, heard };
}

/**
 * Answers the scripts the page adds to `<head>` without fetching them: `install` stands in for the script running,
 * then the script fires `outcome`. Returns the scripts added.
 */
function serveScripts(install: () => void, outcome: 'load' | 'error' = 'load') {
	const scripts: HTMLScriptElement[] = [];

	vi.spyOn(document.head, 'append').mockImplementation((...nodes) => {
		for (const node of nodes) {
			if (!(node instanceof HTMLScriptElement)) continue;
			scripts.push(node);
			setTimeout(() => {
				if (outcome === 'load') install();
				node.dispatchEvent(new Event(outcome));
			});
		}
	});

	return scripts;
}

/** A Mixcloud player widget that answers at once, as its widget API describes. */
function fakeMixcloud() {
	const handlers = new Map<string, Set<(...args: number[]) => void>>();
	const seeked: number[] = [];
	let played = 0;

	const events = (name: string) => {
		const set = handlers.get(name) ?? new Set();
		handlers.set(name, set);

		return {
			on: (handler: (...args: number[]) => void) => set.add(handler),
			off: (handler: (...args: number[]) => void) => set.delete(handler)
		};
	};

	const widget = {
		ready: Promise.resolve(),
		events: { progress: events('progress'), play: events('play'), pause: events('pause'), ended: events('ended') },
		play: async () => {
			played++;
		},
		seek: async (seconds: number) => {
			seeked.push(seconds);

			return true;
		}
	};

	return {
		widget,
		seeked,
		get played() {
			return played;
		},
		emit: (name: string, ...args: number[]) => handlers.get(name)?.forEach((handler) => handler(...args))
	};
}

/** SoundCloud's widget API, with a player that's ready as soon as the page asks and reports seeks as progress. */
function fakeSoundCloud() {
	const handlers = new Map<string, (event?: { currentPosition: number }) => void>();
	const Events = { READY: 'ready', PLAY: 'play', PAUSE: 'pause', FINISH: 'finish', PLAY_PROGRESS: 'playProgress' };

	const Widget = Object.assign(
		() => ({
			bind: (event: string, handler: (event?: { currentPosition: number }) => void) => {
				handlers.set(event, handler);

				if (event === Events.READY) handler();
			},
			unbind: (event: string) => handlers.delete(event),
			play: () => handlers.get(Events.PLAY)?.(),
			seekTo: (milliseconds: number) => handlers.get(Events.PLAY_PROGRESS)?.({ currentPosition: milliseconds })
		}),
		{ Events }
	);

	return { Widget, emit: (event: string, data?: { currentPosition: number }) => handlers.get(event)?.(data) };
}
