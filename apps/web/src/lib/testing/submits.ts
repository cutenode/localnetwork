import { expect, fn, userEvent, within } from 'storybook/test';

/**
 * Stands in for `$app/forms`' enhance in every story (.storybook/preview.ts): Storybook's SvelteKit mocks call it with
 * the submit event in place of posting. Storybook clears its calls before each story.
 */
export const enhanced = fn().mockName('enhance');

/** Presses the button named `name`, checks its form was submitted once, and returns what it posts under each of `fields`. */
export async function submit(canvasElement: HTMLElement, name: string | RegExp, ...fields: string[]) {
	const button = within(canvasElement).getByRole('button', { name });

	await userEvent.click(button);
	await expect(enhanced).toHaveBeenCalledOnce();

	const form = button.closest('form');

	if (!form) throw new Error(`The ${String(name)} button isn't in a form`);

	const data = new FormData(form);

	return Object.fromEntries(fields.map((field) => [field, data.getAll(field)]));
}
