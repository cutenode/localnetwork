<script lang="ts">
	import { Select as SelectPrimitive } from "bits-ui";

	/**
	 * `value` is a string or a string[] depending on `type`, which TS can't
	 * follow once they are destructured apart. Upstream casts the binding
	 * (`value as never`); svelte-check drops a cast on a `bind:` in JS, so
	 * the spread below is cast instead, loosening `value` and nothing else.
	 */
	let {
		open = $bindable(false),
		value = $bindable(),
		...restProps
	}: SelectPrimitive.RootProps = $props();
</script>

<SelectPrimitive.Root bind:open bind:value={value} {...restProps as SelectPrimitive.RootProps & { value?: any }} />