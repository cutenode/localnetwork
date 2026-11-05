import type { Component } from 'svelte';
import { render } from 'vitest-browser-svelte';

/** Renders fields that a page puts in its own form into a form of their own, and returns the form and the render. */
export async function renderInForm<Props extends Record<string, unknown>>(fields: Component<Props>, props: Props) {
	const form = document.body.appendChild(document.createElement('form'));

	return { form, ...(await render(fields, { target: form, props })) };
}

/** What `form` posts under each of `names`. */
export function posted(form: HTMLFormElement, ...names: string[]) {
	const data = new FormData(form);

	return Object.fromEntries(names.map((name) => [name, data.getAll(name)]));
}
