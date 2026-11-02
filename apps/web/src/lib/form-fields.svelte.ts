/**
 * A copy of `values` for a form's controls to `bind:` to, so what someone typed or ticked before the page hydrated
 * stays. Declare it as `const fields = $derived(formFields(values))` so the form starts over from new values, like
 * after a navigation. A form with a single field can bind to a writable `$derived` of its value instead.
 *
 * Hydrating writes a one-way `value={...}` or `checked={...}` over what the control holds
 * (https://github.com/sveltejs/svelte/issues/18951), where `bind:value`, `bind:checked` and `bind:group` take it
 * instead. A `<select>`'s bindings overwrite it too (https://github.com/sveltejs/svelte/issues/18928), so mark its
 * options `selected={...}` instead.
 */
export function formFields<T extends object>(values: T): T {
	// SAFETY: a snapshot of plain form values is the same shape as the values, as a deep copy.
	const fields = $state($state.snapshot(values) as T);

	return fields;
}
