<script lang="ts">
	import { X } from '@lucide/svelte';
	import { KNOWN_GENRES, normalizeGenre } from '#lib/taxonomy/genres.ts';

	let { name = 'genres', value = '', max = 12, id = 'genres', placeholder = 'Type a genre and press Enter' }: { name?: string; value?: string; max?: number; id?: string; placeholder?: string } = $props();

	let tags = $derived(
		value
			.split(',')
			.map((item) => normalizeGenre(item))
			.filter(Boolean)
	);

	let draft = $state('');

	let enhanced = $state(false);

	$effect(() => {
		enhanced = true;
	});

	function add(raw: string) {
		const next = [...tags];

		for (const part of raw.split(',')) {
			const genre = normalizeGenre(part);

			if (genre && !next.includes(genre) && next.length < max) next.push(genre);
		}

		tags = next;
		draft = '';
	}

	function onkeydown(event: KeyboardEvent) {
		if (event.key === 'Enter' || event.key === ',') {
			event.preventDefault();
			add(draft);
		} else if (event.key === 'Backspace' && !draft && tags.length) {
			tags = tags.slice(0, -1);
		}
	}
</script>

{#if enhanced}
	<div class="input flex min-h-10 flex-wrap items-center gap-1.5 px-2 py-1.5 focus-within:ring-2 focus-within:ring-ansi-yellow">
		{#each tags as tag (tag)}
			<span class="chip-accent">
				{tag}
				<button type="button" class="hover:bg-ansi-bright-magenta hover:text-ansi-black" aria-label="Remove {tag}" onclick={() => (tags = tags.filter((t) => t !== tag))}><X class="size-3" /></button>
			</span>
		{/each}
		<input
			{id}
			class="min-w-32 flex-1 border-0 bg-transparent p-0.5 text-base text-ansi-white placeholder:text-ansi-haze focus:ring-0"
			list="{id}-options"
			bind:value={draft}
			{onkeydown}
			onblur={() => draft && add(draft)}
			placeholder={tags.length ? '' : placeholder}
			autocomplete="off"
		/>
	</div>
	<input type="hidden" {name} value={tags.join(', ')} />
{:else}
	<input {id} class="input" {name} {value} list="{id}-options" placeholder="house, techno, disco" />
{/if}
<datalist id="{id}-options">
	{#each KNOWN_GENRES as genre (genre)}<option value={genre}></option>{/each}
</datalist>
