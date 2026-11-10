/**
 * The state behind a text input that suggests as someone types: the suggestions, whether the list is open, and which
 * one the arrow keys are on. The input keeps focus and points at that option with aria-activedescendant. Arrows move,
 * Enter picks the active one (and only then, so Enter on its own still submits the form), Escape and leaving the
 * input close the list. `find` answers a query, and a newer keystroke drops the answer to an older one.
 */
export class Combobox<T> {
	items = $state<T[]>([]);

	open = $state(false);

	active = $state(-1);

	/** Whether an answer is on its way. */
	searching = $state(false);

	#timer: ReturnType<typeof setTimeout> | undefined;

	#pending: AbortController | undefined;

	readonly #find: (query: string, signal: AbortSignal) => Promise<T[]>;

	readonly #pick: (item: T) => void;

	readonly #delay: number;

	/** `pick` takes the chosen item; `delay` is the pause in typing to wait for before asking. */
	constructor(find: (query: string, signal: AbortSignal) => Promise<T[]>, pick: (item: T) => void, delay = 200) {
		this.#find = find;
		this.#pick = pick;
		this.#delay = delay;
	}

	/** Asks for `query` after a pause in typing; under two characters it asks nothing and closes the list. */
	search(query: string) {
		clearTimeout(this.#timer);
		this.#pending?.abort();
		this.active = -1;

		if (query.trim().length < 2) {
			this.items = [];
			this.open = false;
			this.searching = false;

			return;
		}

		this.#timer = setTimeout(() => void this.#ask(query), this.#delay);
	}

	async #ask(query: string) {
		const controller = new AbortController();
		this.#pending = controller;
		this.searching = true;

		try {
			const found = await this.#find(query, controller.signal);

			if (controller.signal.aborted) return;
			this.items = found;
			this.open = found.length > 0;
		} catch {
			// An aborted or failed search leaves the field as it is: what was typed still goes with the form.
		} finally {
			if (!controller.signal.aborted) this.searching = false;
		}
	}

	choose = (item: T) => {
		this.#pick(item);
		this.close();
	};

	close = () => {
		this.open = false;
		this.active = -1;
	};

	onkeydown = (event: KeyboardEvent) => {
		const count = this.items.length;

		if ((event.key === 'ArrowDown' || event.key === 'ArrowUp') && count) {
			event.preventDefault();
			this.open = true;
			this.active =
				event.key === 'ArrowDown' ? (this.active + 1) % count : this.active <= 0 ? count - 1 : this.active - 1;
		} else if (event.key === 'Enter' && this.open && this.active >= 0) {
			event.preventDefault();
			this.choose(this.items[this.active]);
		} else if (event.key === 'Escape' && this.open) {
			event.preventDefault();
			this.close();
		}
	};
}
