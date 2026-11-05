import { afterEach, describe, expect, it, vi } from 'vitest';
import { hotkeys, hotkeyTarget, loadHotkeyPreference, setHotkeysEnabled } from './hotkeys.svelte.ts';

const map = { s: '/shows', p: '/projects' };

afterEach(() => {
	vi.restoreAllMocks();
	localStorage.removeItem('ln:hotkeys');
	hotkeys.enabled = true;
	document.body.replaceChildren();
});

describe('setHotkeysEnabled', () => {
	it('remembers the choice for the next page view', () => {
		// Act
		setHotkeysEnabled(false);
		hotkeys.enabled = true;
		loadHotkeyPreference();

		// Assert
		expect(hotkeys.enabled).toBe(false);
	});

	it('turns them back on for the next page view', () => {
		// Arrange
		setHotkeysEnabled(false);

		// Act
		setHotkeysEnabled(true);
		loadHotkeyPreference();

		// Assert
		expect(hotkeys.enabled).toBe(true);
	});

	it('still switches them off for this page when storage is blocked', () => {
		// Arrange
		blockStorage();

		// Act
		setHotkeysEnabled(false);

		// Assert
		expect(hotkeys.enabled).toBe(false);
	});
});

describe('loadHotkeyPreference', () => {
	it('leaves them on when storage is blocked', () => {
		// Arrange
		hotkeys.enabled = false;
		blockStorage();

		// Act
		loadHotkeyPreference();

		// Assert
		expect(hotkeys.enabled).toBe(true);
	});
});

describe('hotkeyTarget', () => {
	it('sends a mapped key to its page, whatever its case', () => {
		// Act
		const targets = [hotkeyTarget(keydown('s'), map), hotkeyTarget(keydown('P'), map)];

		// Assert
		expect(targets).toEqual(['/shows', '/projects']);
	});

	it('leaves unmapped keys alone', () => {
		expect(hotkeyTarget(keydown('x'), map)).toBeNull();
	});

	it('leaves keys alone while shortcuts are off', () => {
		// Arrange
		hotkeys.enabled = false;

		// Act
		const target = hotkeyTarget(keydown('s'), map);

		// Assert
		expect(target).toBeNull();
	});

	it('leaves a key another handler already took', () => {
		// Arrange
		const event = keydown('s', { cancelable: true });
		event.preventDefault();

		// Act
		const target = hotkeyTarget(event, map);

		// Assert
		expect(target).toBeNull();
	});

	it('leaves typing in fields alone', () => {
		// Arrange
		const fields = ['<input>', '<textarea></textarea>', '<select></select>', '<div contenteditable><b>bold</b></div>'];

		// Act
		const targets = fields.map((html) => hotkeyTarget(keydownIn(html), map));

		// Assert
		expect(targets).toEqual([null, null, null, null]);
	});

	it('still works inside an element that switched editing off', () => {
		expect(hotkeyTarget(keydownIn('<div contenteditable="false"><b>bold</b></div>'), map)).toBe('/shows');
	});

	it('still works with focus on a button', () => {
		expect(hotkeyTarget(keydownIn('<button>Go</button>'), map)).toBe('/shows');
	});

	it('never acts on a held or modified key', () => {
		// Every mix of the flags with at least one set (15 of them), on each key. Checked one by one rather than
		// sampled with fast-check, which the client project doesn't pre-bundle, so importing it here reloads the run.
		const flags = ['repeat', 'ctrlKey', 'metaKey', 'altKey'] as const;

		const mixes = Array.from({ length: 15 }, (_, i) =>
			Object.fromEntries(flags.map((flag, bit) => [flag, Boolean((i + 1) & (1 << bit))]))
		);

		for (const key of ['s', 'p', 'S'])
			for (const init of mixes)
				expect(hotkeyTarget(keydown(key, init), map), `${key} ${JSON.stringify(init)}`).toBeNull();
	});
});

// Helpers

function keydown(key: string, init: KeyboardEventInit = {}) {
	return new KeyboardEvent('keydown', { key, ...init });
}

/** A keydown on the innermost element of `html`, as the browser sends it. */
function keydownIn(html: string) {
	document.body.innerHTML = html;
	let target = document.body.firstElementChild!;

	while (target.firstElementChild) target = target.firstElementChild;
	const event = keydown('s', { bubbles: true });

	target.dispatchEvent(event);

	return event;
}

function blockStorage() {
	const blocked = () => {
		throw new DOMException('Blocked', 'SecurityError');
	};

	vi.spyOn(Storage.prototype, 'getItem').mockImplementation(blocked);
	vi.spyOn(Storage.prototype, 'setItem').mockImplementation(blocked);
}
