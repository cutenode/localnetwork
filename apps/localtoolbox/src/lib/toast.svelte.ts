// The little messages that slide in at the bottom of the popup: "Copied",
// "Nothing imported", and the like.
//
// A panel says what happened and forgets about it; the toaster shows it for a
// few seconds and takes it away. Anything that must stay on screen until it is
// read (why a page could not be used, what an import did) is a note in the
// panel itself, not one of these.
//
// Something going wrong stays up longer than something going right, since it
// is the one worth reading, and no toast counts down while it is under the
// pointer or holding focus (Toaster.svelte).

type Tone = 'ok' | 'bad' | 'info';

interface Toast {
	id: number;
	message: string;
	tone: Tone;
}

/** How long a toast stays up when nobody says otherwise. */
const STAYS = { ok: 2600, info: 2600, bad: 6000 };

/** A toast's countdown: the timer, or how much was left when it was held. */
interface Countdown {
	timer: ReturnType<typeof setTimeout> | null;
	left: number;
	since: number;
}

let nextId = 0;

class Toasts {
	#items: Toast[] = $state([]);

	#countdowns = new Map<number, Countdown>();

	get items() {
		return this.#items;
	}

	/**
	 * @param ms how long it stays up
	 * @returns its id, for `dismiss`
	 */
	show(message: string, tone: Tone = 'info', ms = STAYS[tone]) {
		const id = ++nextId;
		this.#items = [...this.#items, { id, message, tone }];
		this.#run(id, ms);

		return id;
	}

	ok = (message: string, ms?: number) => this.show(message, 'ok', ms);
	bad = (message: string, ms?: number) => this.show(message, 'bad', ms);
	info = (message: string, ms?: number) => this.show(message, 'info', ms);

	/**
	 * Says whether something worked, in one call: `toasts.said(copied, …)`.
	 */
	said(worked: boolean, good: string, bad: string) {
		return worked ? this.ok(good) : this.bad(bad);
	}

	/** Stops a toast's countdown while it is being read. */
	hold(id: number) {
		const countdown = this.#countdowns.get(id);

		if (!countdown?.timer) return;
		clearTimeout(countdown.timer);
		countdown.timer = null;
		countdown.left -= Date.now() - countdown.since;
	}

	/** Starts it again with what it had left, and never less than a second to finish reading. */
	release(id: number) {
		const countdown = this.#countdowns.get(id);

		if (!countdown || countdown.timer) return;
		this.#run(id, Math.max(countdown.left, 1000));
	}

	dismiss(id: number) {
		const countdown = this.#countdowns.get(id);

		if (countdown?.timer) clearTimeout(countdown.timer);
		this.#countdowns.delete(id);
		this.#items = this.#items.filter((item) => item.id !== id);
	}

	#run(id: number, ms: number) {
		this.#countdowns.set(id, { timer: setTimeout(() => this.dismiss(id), ms), left: ms, since: Date.now() });
	}
}

export const toasts = new Toasts();
