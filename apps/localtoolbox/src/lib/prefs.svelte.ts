// Panel options, remembered in the popup's own localStorage.
//
// The extension asks for no storage permission on the user's behalf to keep
// these: they are the popup page's own, they never leave it, and losing them
// costs a re-tick of a checkbox. The keys are the ones the exported state file
// carries (see state-transfer.ts and the schema in docs/), so they are part of
// the format and must not be renamed.
//
// Values are written as "1"/"0" for a flag, which is what the export format
// expects, and read leniently: an older install wrote "true"/"false".

/** An option: `.value` reads it and writes it through, so `bind:` works on it. */
export interface Pref<T> {
	value: T;
	/** Re-reads what is stored. */
	reload: () => void;
}

const read = (key: string) => {
	try {
		return localStorage.getItem(key);
	} catch (_e) {
		// Storage unavailable — the option falls back to its default.
		return null;
	}
};

const write = (key: string, value: string) => {
	try {
		localStorage.setItem(key, value);
	} catch (_e) {
		// Ignore: an option that cannot be remembered still works this time.
	}
};

// Every option made in this popup, so that one call can put them all back in
// front of whoever is showing them.
//
// An import writes straight to localStorage (state-transfer.js), which no
// `$state` follows — and the panel whose sort order or "hide downloaded" the
// file just set is *still on screen*, under the settings sheet the import was
// run from. Before, the Manager panel reloaded its own two by hand, because
// the import was in that panel's own footer and there was nothing else it
// could have changed. Now it is in settings, over any tool, so there is
// nowhere left that knows which options an import touched — and a list is.
//
// It grows by one per option a panel makes, and never shrinks: reloading one
// belonging to a panel that has since been closed re-reads a string and sets a
// value nothing is watching. The whole list dies with the popup, which is
// measured in minutes.
const made: Pref<unknown>[] = [];

/** Re-reads every option from storage, for an import that may have set some. */
export function reloadPrefs() {
	for (const pref of made) pref.reload();
}

/**
 * A `$state` value that writes itself to localStorage whenever it is set.
 *
 * `reload()` re-reads the stored value — `reloadPrefs()` above calls it on
 * every option after an import, which may have replaced them underneath.
 *
 * @param key the localStorage key, which the state file names too
 * @param initial what it is while nothing (or nothing `parse` takes) is stored
 * @param options `parse` turns what is stored back into a value; without one, the stored string
 *   is taken as it is, which is only right for a string option
 */
export function persisted(
	key: string,
	initial: string,
	options?: { serialize?: (value: string) => string }
): Pref<string>;
export function persisted<T>(
	key: string,
	initial: T,
	options: { parse: (raw: string, fallback: T) => T; serialize?: (value: T) => string }
): Pref<T>;
export function persisted<T>(
	key: string,
	initial: T,
	{
		// SAFETY: the overloads above only let `parse` be left out of a string option.
		parse = (raw: string) => raw as T,
		serialize = String
	}: { parse?: (raw: string, fallback: T) => T; serialize?: (value: T) => string } = {}
): Pref<T> {
	const stored = () => {
		const raw = read(key);

		return raw === null ? initial : parse(raw, initial);
	};

	let value = $state(stored());

	const pref = {
		get value() {
			return value;
		},
		set value(next) {
			value = next;
			write(key, serialize(next));
		},
		reload() {
			value = stored();
		}
	};

	made.push(pref);

	return pref;
}

export const flagPref = (key: string, initial: boolean) =>
	persisted(key, initial, {
		parse: (raw) => raw === '1' || raw === 'true',
		serialize: (value) => (value ? '1' : '0')
	});

/**
 * A string that is one of `values`; anything else stored reads as `initial`. Only the read is
 * checked: a write is taken as it comes.
 */
export const choicePref = (key: string, initial: string, values: string[]) =>
	persisted(key, initial, {
		parse: (raw, fallback) => (values.includes(raw) ? raw : fallback)
	});

export const textPref = (key: string, initial = '') =>
	persisted(key, initial, { serialize: (value) => String(value).trim() });
