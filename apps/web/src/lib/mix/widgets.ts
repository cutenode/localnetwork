/**
 * Mixcloud's and SoundCloud's widget APIs, so a set's tracklist can follow their players and seek them. Each script
 * loads the first time someone opens one of their players, from the host the CSP's script-src allows. YouTube's
 * player has an API too, but it loads scripts from more hosts than this is worth, so its player plays on its own.
 */

/** A platform player the page follows: where it is, and whether it's playing. */
export type WidgetEvents = { time(seconds: number): void; playing(playing: boolean): void };

/** What the page can do with a player it follows. */
export type Widget = { seek(seconds: number): void; destroy(): void };

type Handlers<T extends unknown[]> = {
	on(handler: (...args: T) => void): void;
	off(handler: (...args: T) => void): void;
};

/** https://www.mixcloud.com/developers/widget/ */
type MixcloudWidget = {
	/** Resolves once the player answers. */
	ready: Promise<unknown>;
	events: {
		progress: Handlers<[position: number, duration: number]>;
		play: Handlers<[]>;
		pause: Handlers<[]>;
		ended: Handlers<[]>;
	};
	play(): Promise<void>;
	seek(seconds: number): Promise<boolean>;
};

/** https://developers.soundcloud.com/docs/api/html5-widget */
type SoundCloudWidget = {
	bind(event: string, handler: (event?: { currentPosition: number }) => void): void;
	unbind(event: string): void;
	play(): void;
	seekTo(milliseconds: number): void;
};

type SoundCloudApi = {
	(iframe: HTMLIFrameElement): SoundCloudWidget;
	Events: { READY: string; PLAY: string; PAUSE: string; FINISH: string; PLAY_PROGRESS: string };
};

declare global {
	interface Window {
		Mixcloud?: { PlayerWidget(iframe: HTMLIFrameElement): MixcloudWidget };
		SC?: { Widget: SoundCloudApi };
	}
}

const MIXCLOUD_API = 'https://widget.mixcloud.com/media/js/widgetApi.js';

const SOUNDCLOUD_API = 'https://w.soundcloud.com/player/api.js';

const loading = new Map<string, Promise<void>>();

/** Loads a widget's script once, unless the page already has its API. */
async function api<T>(src: string, get: () => T | undefined): Promise<T> {
	const found = get();

	if (found) return found;

	let load = loading.get(src);

	if (!load) {
		load = new Promise<void>((resolve, reject) => {
			const script = document.createElement('script');
			script.src = src;
			script.async = true;
			script.addEventListener('load', () => resolve(), { once: true });
			script.addEventListener('error', () => reject(new Error(`could not load ${src}`)), { once: true });
			document.head.append(script);
		});
		// A failed load can be tried again the next time a player opens.
		load.catch(() => loading.delete(src));
		loading.set(src, load);
	}

	await load;
	const loaded = get();

	if (!loaded) throw new Error(`${src} has no API`);

	return loaded;
}

async function mixcloud(iframe: HTMLIFrameElement, on: WidgetEvents): Promise<Widget> {
	const widget = (await api(MIXCLOUD_API, () => window.Mixcloud)).PlayerWidget(iframe);
	await widget.ready;

	const progress = (position: number) => on.time(position);
	const play = () => on.playing(true);
	const stop = () => on.playing(false);

	widget.events.progress.on(progress);
	widget.events.play.on(play);
	widget.events.pause.on(stop);
	widget.events.ended.on(stop);

	return {
		// Mixcloud only seeks a mix that's playing.
		seek: (seconds) => void widget.play().then(() => widget.seek(seconds)),
		destroy: () => {
			widget.events.progress.off(progress);
			widget.events.play.off(play);
			widget.events.pause.off(stop);
			widget.events.ended.off(stop);
		}
	};
}

async function soundcloud(iframe: HTMLIFrameElement, on: WidgetEvents): Promise<Widget> {
	const SoundCloud = await api(SOUNDCLOUD_API, () => window.SC?.Widget);
	const widget = SoundCloud(iframe);
	const { READY, PLAY, PAUSE, FINISH, PLAY_PROGRESS } = SoundCloud.Events;

	await new Promise<void>((resolve) => widget.bind(READY, () => resolve()));
	widget.bind(PLAY_PROGRESS, (event) => on.time((event?.currentPosition ?? 0) / 1000));
	widget.bind(PLAY, () => on.playing(true));
	widget.bind(PAUSE, () => on.playing(false));
	widget.bind(FINISH, () => on.playing(false));

	return {
		seek: (seconds) => {
			widget.seekTo(seconds * 1000);
			widget.play();
		},
		destroy: () => [READY, PLAY, PAUSE, FINISH, PLAY_PROGRESS].forEach((event) => widget.unbind(event))
	};
}

const followers = new Map([
	['Mixcloud', mixcloud],
	['SoundCloud', soundcloud]
]);

/** Follows the player in `iframe`, or resolves null for a platform whose player can't be followed. */
export function follow(platform: string, iframe: HTMLIFrameElement, on: WidgetEvents): Promise<Widget | null> {
	return followers.get(platform)?.(iframe, on) ?? Promise.resolve(null);
}
