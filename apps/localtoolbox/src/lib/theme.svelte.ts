// Light or dark, following the browser unless the user says otherwise.
//
// The choice is the popup's own (localStorage, no permission), and is applied
// by putting `dark` on <html> — the class shadcn-svelte's components are
// written against.

const KEY = 'pref:theme';

const MODES = ['system', 'light', 'dark'] as const;

type ThemeMode = (typeof MODES)[number];

/** Whether a stored or given string is one of the modes. Nothing stored is null, which is not. */
const isMode = (value: string | null): value is ThemeMode => MODES.some((mode) => mode === value);

const query = () => (typeof matchMedia === 'undefined' ? null : matchMedia('(prefers-color-scheme: dark)'));

class Theme {
	#mode: ThemeMode = $state('system');
	#systemDark = $state(false);

	get mode() {
		return this.#mode;
	}

	get dark() {
		return this.#mode === 'system' ? this.#systemDark : this.#mode === 'dark';
	}

	/** Reads the saved choice and starts following the browser's setting. */
	start() {
		try {
			const saved = localStorage.getItem(KEY);

			if (isMode(saved)) this.#mode = saved;
		} catch (_e) {
			// Storage unavailable — follow the browser.
		}

		const media = query();

		if (media) {
			this.#systemDark = media.matches;
			// The browser can change its mind while the popup is open.
			media.addEventListener('change', (event) => {
				this.#systemDark = event.matches;
				this.apply();
			});
		}

		this.apply();
	}

	/** Cycles system → light → dark → system, which is the header button. */
	next() {
		this.set(MODES[(MODES.indexOf(this.#mode) + 1) % MODES.length]);
	}

	/** @param mode anything but one of the three is taken as 'system' */
	set(mode: string) {
		this.#mode = isMode(mode) ? mode : 'system';

		try {
			localStorage.setItem(KEY, this.#mode);
		} catch (_e) {
			// Ignore: the choice holds for this opening of the popup.
		}

		this.apply();
	}

	apply() {
		document.documentElement.classList.toggle('dark', this.dark);
	}
}

export const theme = new Theme();
