<script lang="ts">
	import { type ActorSuggestion, searchActors } from '#lib/actors.ts';
	import { Combobox } from '#lib/combobox.svelte.ts';
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

	// The search can be swapped in tests and stories, so it's read when it's used.
	const accounts = new Combobox<ActorSuggestion>((query, signal) => search(query, signal), (actor) => (handle = actor.handle));
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
			aria-expanded={accounts.open}
			aria-controls={listId}
			aria-activedescendant={accounts.open && accounts.active >= 0 ? `${listId}-${accounts.active}` : undefined}
			aria-invalid={invalid || undefined}
			aria-describedby={describedby}
			bind:value={handle}
			oninput={(event) => accounts.search(event.currentTarget.value.trim().replace(/^@/, ''))}
			onkeydown={accounts.onkeydown}
			onblur={accounts.close}
			placeholder="alice.bsky.social"
			autocomplete="username"
			autocapitalize="none"
			spellcheck="false"
			required
		/>
		<ul id={listId} role="listbox" aria-label="Accounts" hidden={!accounts.open} class="absolute inset-x-0 top-full z-20 m-0 mt-1 list-none rounded-lg border border-line-strong bg-raised p-1">
			{#each accounts.items as actor, i (actor.did)}
				<!-- The input keeps focus and handles the keys (aria-activedescendant), so the options only take the pointer. -->
				<!-- svelte-ignore a11y_click_events_have_key_events -->
				<li
					id="{listId}-{i}"
					role="option"
					aria-selected={i === accounts.active}
					class="flex cursor-pointer items-center gap-3 rounded-md px-2.5 py-2 aria-selected:bg-well"
					onpointerdown={(event) => event.preventDefault()}
					onpointermove={() => (accounts.active = i)}
					onclick={() => accounts.choose(actor)}
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
	<p class="sr-only" role="status">{accounts.open ? `${accounts.items.length} ${accounts.items.length === 1 ? 'account' : 'accounts'} found. Use the arrow keys to pick one.` : ''}</p>
</div>
