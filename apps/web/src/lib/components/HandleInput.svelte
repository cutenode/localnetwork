<script lang="ts">
	import { type ActorSuggestion, searchActors } from '#lib/actors.ts';
	import Avatar from './Avatar.svelte';

	/**
	 * The sign-in handle field, with a fixed "@" in front. As someone types, up to five matching accounts open below it
	 * as a combobox: arrows move through them, Enter or a click fills the handle, Escape closes the list. Nothing is
	 * picked until they choose, so Enter on its own still submits what they typed. `search` finds the accounts.
	 */
	let {
		value = '',
		invalid = false,
		describedby,
		search = searchActors
	}: { value?: string; invalid?: boolean; describedby?: string; search?: (query: string, signal: AbortSignal) => Promise<ActorSuggestion[]> } = $props();

	const id = $props.id();

	const listId = `${id}-accounts`;

	// Bound, so a handle typed before the page hydrated stays (see formFields).
	let handle = $derived(value);

	let suggestions = $state<ActorSuggestion[]>([]);

	let open = $state(false);

	let active = $state(-1);

	let timer: ReturnType<typeof setTimeout> | undefined;

	let pending: AbortController | undefined;

	/** Waits for a pause in typing, then asks once, dropping any answer a newer keystroke has made stale. */
	function oninput(event: Event & { currentTarget: HTMLInputElement }) {
		clearTimeout(timer);
		pending?.abort();
		active = -1;
		const query = event.currentTarget.value.trim().replace(/^@/, '');

		if (query.length < 2) {
			suggestions = [];
			open = false;

			return;
		}

		timer = setTimeout(async () => {
			const controller = new AbortController();
			pending = controller;

			try {
				const found = await search(query, controller.signal);

				if (controller.signal.aborted) return;
				suggestions = found;
				open = found.length > 0;
			} catch {
				// An aborted or failed search leaves the field as it is: the handle can still be typed in full.
			}
		}, 200);
	}

	function choose(actor: ActorSuggestion) {
		handle = actor.handle;
		open = false;
		active = -1;
	}

	function onkeydown(event: KeyboardEvent) {
		const count = suggestions.length;

		if ((event.key === 'ArrowDown' || event.key === 'ArrowUp') && count) {
			event.preventDefault();
			open = true;
			active = event.key === 'ArrowDown' ? (active + 1) % count : active <= 0 ? count - 1 : active - 1;
		} else if (event.key === 'Enter' && open && active >= 0) {
			event.preventDefault();
			choose(suggestions[active]);
		} else if (event.key === 'Escape' && open) {
			event.preventDefault();
			open = false;
			active = -1;
		}
	}

	function onblur() {
		open = false;
		active = -1;
	}
</script>

<div>
	<label class="label" for={id}>Your handle</label>
	<div class="relative">
		<span aria-hidden="true" class="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-fg-subtle">@</span>
		<input
			{id}
			name="handle"
			class="input pl-7.5"
			role="combobox"
			aria-autocomplete="list"
			aria-expanded={open}
			aria-controls={listId}
			aria-activedescendant={open && active >= 0 ? `${listId}-${active}` : undefined}
			aria-invalid={invalid || undefined}
			aria-describedby={describedby}
			bind:value={handle}
			{oninput}
			{onkeydown}
			{onblur}
			placeholder="alice.bsky.social"
			autocomplete="username"
			autocapitalize="none"
			spellcheck="false"
			required
		/>
		<ul id={listId} role="listbox" aria-label="Accounts" hidden={!open} class="absolute inset-x-0 top-full z-20 m-0 mt-1 list-none rounded-lg border border-line-strong bg-raised p-1">
			{#each suggestions as actor, i (actor.did)}
				<!-- The input keeps focus and handles the keys (aria-activedescendant), so the options only take the pointer. -->
				<!-- svelte-ignore a11y_click_events_have_key_events -->
				<li
					id="{listId}-{i}"
					role="option"
					aria-selected={i === active}
					class="flex cursor-pointer items-center gap-3 rounded-md px-2.5 py-2 aria-selected:bg-well"
					onpointerdown={(event) => event.preventDefault()}
					onpointermove={() => (active = i)}
					onclick={() => choose(actor)}
				>
					<Avatar src={actor.avatar} name={actor.displayName ?? actor.handle} size="sm" />
					<span class="flex min-w-0 flex-col leading-tight">
						<span class="truncate text-small font-semibold text-fg">{actor.displayName ?? actor.handle}</span>
						<span class="truncate text-label text-fg-muted">@{actor.handle}</span>
					</span>
				</li>
			{/each}
		</ul>
	</div>
	<p class="sr-only" role="status">{open ? `${suggestions.length} ${suggestions.length === 1 ? 'account' : 'accounts'} found. Use the arrow keys to pick one.` : ''}</p>
</div>
