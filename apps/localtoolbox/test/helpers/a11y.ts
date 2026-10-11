// A small accessibility audit, run over whatever a panel has drawn.
//
// Not a substitute for a real one — no browser here computes a layout, and a
// contrast ratio cannot be measured in jsdom. What it does check is the part
// that breaks silently and is invisible to every other test in this suite: an
// icon-only button with no name, a label pointing at an id that is not there,
// a toggle whose state is only a colour, an id used twice because two rows
// rendered the same component.
//
// `audit(root)` returns a list of problems, each naming the element and what
// is wrong with it. An empty list is the assertion.

const INTERACTIVE = [
	'button',
	'a[href]',
	'input:not([type="hidden"])',
	'select',
	'textarea',
	'[role="button"]',
	'[role="link"]',
	'[role="checkbox"]',
	'[role="switch"]',
	'[role="menuitem"]',
	'[role="option"]',
	'[role="combobox"]',
	'[role="tab"]'
].join(', ');

// Roles that carry a state attribute, and the attribute each may carry.
const STATE_ROLES = {
	'aria-pressed': ['button'],
	'aria-checked': ['checkbox', 'switch', 'radio', 'menuitemcheckbox', 'option'],
	'aria-expanded': ['button', 'combobox', 'link', 'menuitem', 'tab'],
	'aria-selected': ['option', 'tab', 'row', 'gridcell']
} satisfies Record<string, string[]>;

const IMPLICIT_ROLE = new Map([
	['BUTTON', 'button'],
	['A', 'link'],
	['SUMMARY', 'button'],
	['SELECT', 'combobox'],
	['TEXTAREA', 'textbox']
]);

// Whether a control is switched off: what a `disabled` property says, on the
// elements that have one.
const isDisabled = (el: Element) => 'disabled' in el && el.disabled === true;

const describe = (el: Element) => {
	// An SVG element's className is an object, not the class list; it goes without.
	const classes =
		el instanceof HTMLElement && el.className ? `.${el.className.trim().split(/\s+/).slice(0, 2).join('.')}` : '';

	const label = el.getAttribute?.('aria-label');

	return `<${el.tagName.toLowerCase()}${classes}${label ? ` aria-label="${label}"` : ''}>`;
};

// Text a screen reader would announce for the element's own content.
function contentName(el: Element): string {
	let out = '';

	// Text nodes as well, which are asked only what every node can answer.
	for (const node of el.childNodes) {
		if (node.nodeType === Node.TEXT_NODE) out += node.textContent;
		else if (node instanceof Element) {
			if (node.getAttribute('aria-hidden') === 'true' || node.hasAttribute('hidden')) continue;

			if (node.tagName === 'IMG') out += node.getAttribute('alt') ?? '';
			else if (node.tagName === 'SVG' || node.tagName === 'svg') {
				out += node.querySelector('title')?.textContent ?? '';
			} else out += contentName(node);
		}
	}

	return out.replace(/\s+/g, ' ').trim();
}

export function accessibleName(el: Element) {
	const doc = el.ownerDocument;
	const labelledby = el.getAttribute('aria-labelledby');

	if (labelledby) {
		const named = labelledby
			.split(/\s+/)
			.map((id) => doc.getElementById(id))
			.filter(Boolean)
			.map((target) => contentName(target!) || target!.getAttribute('aria-label') || '')
			.join(' ')
			.trim();

		if (named) return named;
	}

	const label = el.getAttribute('aria-label');

	if (label?.trim()) return label.trim();

	if (el.id) {
		const explicit = doc.querySelector(`label[for="${CSS.escape(el.id)}"]`);

		if (explicit && contentName(explicit)) return contentName(explicit);
	}

	const wrapping = el.closest('label');

	if (wrapping && contentName(wrapping)) return contentName(wrapping);

	if (el.getAttribute('title')?.trim()) return el.getAttribute('title')!.trim();
	const content = contentName(el);

	if (content) return content;

	if (el.tagName === 'INPUT' && el.getAttribute('placeholder')?.trim()) {
		return el.getAttribute('placeholder')!.trim();
	}

	return '';
}

const hiddenFromAssistiveTech = (el: Element) =>
	!!el.closest('[aria-hidden="true"], [hidden], [inert]') ||
	// bits-ui keeps a closed popover in the DOM, marked shut. It marks the
	// *trigger* of a closed tooltip or select the same way — and those are the
	// icon-only buttons this audit is most for — so a trigger is never taken
	// for the content it opens.
	!!el.closest('[data-state="closed"]:not([data-tooltip-trigger]):not([aria-haspopup]):not([aria-expanded])');

export function audit(root: ParentNode = document.body) {
	const problems: string[] = [];
	const report = (el: Element, what: string) => problems.push(`${describe(el)} ${what}`);

	// 1. Every control has a name.
	for (const el of root.querySelectorAll(INTERACTIVE)) {
		if (hiddenFromAssistiveTech(el)) continue;

		if (el instanceof HTMLInputElement && ['submit', 'button', 'reset'].includes(el.type)) continue;

		if (!accessibleName(el)) report(el, 'has no accessible name');
	}

	// 2. Ids are unique, and every reference resolves.
	const seen = new Map<string, number>();

	for (const el of root.querySelectorAll('[id]')) {
		const count = (seen.get(el.id) ?? 0) + 1;
		seen.set(el.id, count);

		if (count === 2) report(el, `reuses the id "${el.id}"`);
	}

	for (const attribute of ['aria-labelledby', 'aria-describedby', 'aria-controls', 'aria-activedescendant']) {
		for (const el of root.querySelectorAll(`[${attribute}]`)) {
			// aria-controls may name an element that only exists while open.
			if (attribute === 'aria-controls') continue;

			for (const id of el.getAttribute(attribute)!.split(/\s+/).filter(Boolean)) {
				if (!el.ownerDocument.getElementById(id)) {
					report(el, `${attribute} points at "${id}", which is not on the page`);
				}
			}
		}
	}

	for (const label of root.querySelectorAll('label[for]')) {
		if (!label.ownerDocument.getElementById(label.getAttribute('for')!)) {
			report(label, `labels "${label.getAttribute('for')}", which is not on the page`);
		}
	}

	// 3. Nothing steals the tab order.
	for (const el of root.querySelectorAll('[tabindex]')) {
		if (Number(el.getAttribute('tabindex')) > 0) report(el, 'has a positive tabindex');
	}

	// 4. Nothing focusable is hidden from assistive technology while still
	//    reachable by keyboard.
	for (const el of root.querySelectorAll('[aria-hidden="true"]')) {
		for (const focusable of el.querySelectorAll('button, a[href], input, select, textarea')) {
			if (focusable.getAttribute('tabindex') === '-1' || isDisabled(focusable)) continue;
			report(focusable, 'is focusable inside an aria-hidden subtree');
		}
	}

	// 5. Images say what they are, or say they are decoration.
	for (const img of root.querySelectorAll('img')) {
		if (img.getAttribute('alt') === null && img.getAttribute('aria-hidden') !== 'true') {
			report(img, 'has no alt attribute');
		}
	}

	// 6. State attributes sit on something that can have that state.
	for (const [attribute, roles] of Object.entries(STATE_ROLES)) {
		for (const el of root.querySelectorAll(`[${attribute}]`)) {
			const role = el.getAttribute('role') ?? IMPLICIT_ROLE.get(el.tagName) ?? '';

			if (!roles.includes(role)) report(el, `has ${attribute} but its role is "${role || 'none'}"`);
			const value = el.getAttribute(attribute)!;

			if (!['true', 'false', 'mixed', 'undefined'].includes(value)) {
				report(el, `has ${attribute}="${value}"`);
			}
		}
	}

	// 7. A disabled control is disabled in the accessibility tree too.
	for (const el of root.querySelectorAll('[aria-disabled="true"]')) {
		if (el instanceof HTMLButtonElement && !el.disabled && el.getAttribute('tabindex') !== '-1') {
			report(el, 'is aria-disabled but still focusable and clickable');
		}
	}

	return problems;
}

/** Elements a keyboard reaches, in order. */
export const focusable = (root: ParentNode = document.body) =>
	[...root.querySelectorAll('button, a[href], input, select, textarea, [tabindex]')].filter(
		(el) => !isDisabled(el) && el.getAttribute('tabindex') !== '-1' && !hiddenFromAssistiveTech(el)
	);
