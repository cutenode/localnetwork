<script lang="ts">
	/**
	 * Text on the page turned into its own field: the same font, size and box as the text it replaces, marked with a
	 * dashed outline drawn outside the box, so switching to edit mode moves nothing. It wraps like the text did and grows
	 * with it. Enter adds a line only when `multiline`; otherwise it's one value that wraps. `label` names it for screen
	 * readers, since the field sits where its text was and has no visible label of its own. `error` is a refused save's
	 * reason, drawn under it.
	 *
	 * Its type comes from `class`, which goes on the field itself: the forms plugin gives every textarea 1rem, so it
	 * can't take its size from the element around it. The box is drawn with the site's own classes, so a page opened at
	 * `?edit` has it before any script runs; the outline that marks it as a field comes with the editor (EditSession).
	 */
	let {
		value = $bindable(''),
		label,
		name,
		placeholder,
		multiline = false,
		required = false,
		readonly = false,
		error,
		class: className = '',
		element = $bindable()
	}: {
		value?: string;
		label: string;
		name: string;
		placeholder?: string;
		multiline?: boolean;
		required?: boolean;
		readonly?: boolean;
		error?: string;
		class?: string;
		element?: HTMLTextAreaElement;
	} = $props();

	const id = $props.id();

	/** A single-line value still wraps, so Enter would only add a line break nobody asked for. */
	function onkeydown(event: KeyboardEvent) {
		if (!multiline && event.key === 'Enter') event.preventDefault();
	}
</script>

<span class="block min-w-0">
	<label for={id} class="sr-only">{label}{required ? ' (required)' : ''}</label>
	<textarea
		{id}
		{name}
		bind:value
		bind:this={element}
		{placeholder}
		{required}
		{readonly}
		{onkeydown}
		rows="1"
		data-inline-field
		class="block w-full resize-none overflow-hidden rounded-xs border-0 bg-transparent p-0 field-sizing-content {className}"
		aria-invalid={error ? 'true' : undefined}
		aria-describedby={error ? `${id}-error` : undefined}
	></textarea>
	{#if error}<span id="{id}-error" class="mt-2 block text-label font-normal tracking-normal text-critical">{error}</span>{/if}
</span>

