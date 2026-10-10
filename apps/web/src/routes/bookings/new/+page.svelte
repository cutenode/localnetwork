<script lang="ts">
	import Avatar from '#lib/components/Avatar.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import GenreInput from '#lib/components/GenreInput.svelte';
	import { placeLabel } from '#lib/format.ts';
	import { FEE_TIERS, SET_TYPES } from '#lib/taxonomy/options.ts';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const v = $derived(form?.values ?? {});
</script>

<svelte:head><title>Inquire with {data.project.profile.name} · Local Network</title></svelte:head>

<div class="container-page max-w-3xl py-10">
	<div class="flex items-center gap-4">
		<Avatar src={data.project.avatarUrl} name={data.project.profile.name} />
		<div>
			<p class="eyebrow">Booking inquiry</p>
			<h1 class="text-3xl">{data.project.profile.name}</h1>
			<p class="muted text-sm">{placeLabel(data.project.profile)}</p>
		</div>
	</div>
	<p class="muted mt-4">This opens a private thread with the project's managers. The details below go first, so they can answer quickly.</p>
	<form method="POST" class="card card-pad mt-8 space-y-5">
		<FormMessage {form} />
		<div>
			<label class="label" for="title">Show name</label>
			<input id="title" name="title" class="input" required maxlength="120" value={v.title ?? ''} placeholder="Saturday Social at The Lot" />
		</div>
		<div class="grid gap-4 sm:grid-cols-3">
			<div><label class="label" for="date">Date</label><input id="date" type="date" name="date" class="input" value={v.date ?? data.date} /></div>
			<div><label class="label" for="time">Set time</label><input id="time" type="time" name="time" class="input" value={v.time ?? '22:00'} /></div>
			<div><label class="label" for="setMinutes">Set length (min)</label><input id="setMinutes" type="number" min="15" max="720" name="setMinutes" class="input" value={v.setMinutes ?? ''} /></div>
		</div>
		<div class="grid gap-4 sm:grid-cols-2">
			<div><label class="label" for="venue">Venue</label><input id="venue" name="venue" class="input" value={v.venue ?? ''} /></div>
			<div><label class="label" for="capacity">Capacity</label><input id="capacity" type="number" min="1" name="capacity" class="input" value={v.capacity ?? ''} /></div>
		</div>
		<div class="grid gap-4 sm:grid-cols-4">
			<div><label class="label" for="budgetAmount">Budget</label><input id="budgetAmount" type="number" min="0" name="budgetAmount" class="input" value={v.budgetAmount ?? ''} /></div>
			<div><label class="label" for="currency">Currency</label><input id="currency" name="currency" class="input uppercase" maxlength="3" value={v.currency ?? 'USD'} /></div>
			<div>
				<label class="label" for="budgetTier">Tier</label>
				<select id="budgetTier" name="budgetTier" class="input"><option value="">—</option>{#each FEE_TIERS as option (option.value)}<option value={option.value} selected={v.budgetTier === option.value}>{option.label}</option>{/each}</select>
			</div>
			<div>
				<label class="label" for="setType">Slot</label>
				<select id="setType" name="setType" class="input"><option value="">—</option>{#each SET_TYPES as option (option.value)}<option value={option.value} selected={v.setType === option.value}>{option.label}</option>{/each}</select>
			</div>
		</div>
		<div><label class="label" for="genres">Sound of the night</label><GenreInput value={v.genres ?? ''} /></div>
		<div>
			<label class="label" for="message">Message</label>
			<textarea id="message" name="message" class="input min-h-36" required maxlength="2000" placeholder="Tell them about the night, the crowd, and what you'd like from them.">{v.message ?? ''}</textarea>
		</div>
		<div class="flex justify-end gap-3">
			<a href="/projects/{data.project.slug}" class="btn-ghost">Cancel</a>
			<button class="btn-accent">Send inquiry</button>
		</div>
	</form>
</div>
