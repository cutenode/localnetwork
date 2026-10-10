/** Single-key BBS shortcuts. They can be switched off from the footer (WCAG 2.1.4). */
export const hotkeys = $state({ enabled: true });

const STORAGE_KEY = 'ln:hotkeys';

export function loadHotkeyPreference() {
	try {
		hotkeys.enabled = localStorage.getItem(STORAGE_KEY) !== 'off';
	} catch {
		hotkeys.enabled = true;
	}
}

export function setHotkeysEnabled(enabled: boolean) {
	hotkeys.enabled = enabled;

	try {
		localStorage.setItem(STORAGE_KEY, enabled ? 'on' : 'off');
	} catch {
		// Storage can be blocked; the toggle still works for this page view.
	}
}

/** The shortcut target for a keydown, or null when the key should be left alone. */
export function hotkeyTarget(event: KeyboardEvent, map: Record<string, string>): string | null {
	if (!hotkeys.enabled || event.defaultPrevented || event.repeat || event.ctrlKey || event.metaKey || event.altKey)
		return null;
	const target = event.target;

	if (
		target instanceof Element &&
		target.closest('input, textarea, select, [contenteditable]:not([contenteditable="false"])')
	)
		return null;

	return map[event.key.toLowerCase()] ?? null;
}
