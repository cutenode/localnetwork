// The little messages that slide in at the bottom of the popup: "Copied",
// "Nothing imported", and the like.
//
// A panel says what happened and forgets about it; the toaster shows it for a
// few seconds and takes it away. Anything that must stay on screen until it is
// read (why a page could not be used, what an import did) is a note in the
// panel itself, not one of these.

type Tone = 'ok' | 'bad' | 'info';

interface Toast {
	id: number;
	message: string;
	tone: Tone;
}

let nextId = 0;

class Toasts {
	#items: Toast[] = $state([]);

	get items() {
		return this.#items;
	}

	/**
	 * @param ms how long it stays up
	 * @returns its id, for `dismiss`
	 */
	show(message: string, tone: Tone = 'info', ms = 2600) {
		const id = ++nextId;
		this.#items = [...this.#items, { id, message, tone }];
		setTimeout(() => this.dismiss(id), ms);

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

	dismiss(id: number) {
		this.#items = this.#items.filter((item) => item.id !== id);
	}
}

export const toasts = new Toasts();
