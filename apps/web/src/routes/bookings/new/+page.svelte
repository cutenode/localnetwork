<script lang="ts">
	import Avatar from '#lib/components/Avatar.svelte';
	import FormMessage from '#lib/components/FormMessage.svelte';
	import GenreInput from '#lib/components/GenreInput.svelte';
	import { placeLabel } from '#lib/format.ts';
	import { FEE_TIERS, SET_TYPES } from '#lib/taxonomy/options.ts';
	import Eyebrow from '#lib/ui/Eyebrow.svelte';
	import type { PageProps } from './$types';

	let { data, form }: PageProps = $props();

	const v = $derived(form?.values ?? {});
</script>

<svelte:head><title>Inquire with {data.project.profile.name} · Local Network</title></svelte:head>

<div class="container-page flex max-w-3xl flex-col gap-8 py-10 sm:py-14">
	<header class="flex flex-col gap-4">
		<div class="flex items-center gap-4">
			<Avatar src={data.project.avatarUrl} name={data.project.profile.name} />
			<div class="flex min-w-0 flex-col gap-1">
				<Eyebrow>Booking inquiry</Eyebrow>
				<h1 class="m-0 text-title font-semibold">{data.project.profile.name}</h1>
				<p class="m-0 text-small text-fg-muted">{placeLabel(data.project.profile)}</p>
			</div>
		</div>
		<p class="m-0 text-lede text-fg-muted">This opens a private thread with the project's managers. The details below go first, so they can answer quickly.</p>
	</header>
	<form method="POST" class="card flex flex-col gap-5 p-5 sm:p-6">
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
		<div class="grid grid-cols-2 gap-4 md:grid-cols-4">
			<div><label class="label" for="budgetAmount">Budget</label><input id="budgetAmount" type="number" min="0" name="budgetAmount" class="input" value={v.budgetAmount ?? ''} /></div>
			<div><label class="label" for="currency">Currency</label><input id="currency" name="currency" class="input font-mono uppercase" maxlength="3" value={v.currency ?? 'USD'} /></div>
			<div>
				<label class="label" for="budgetTier">Tier</label>
				<select id="budgetTier" name="budgetTier" class="input"><option value="">Not set</option>{#each FEE_TIERS as option (option.value)}<option value={option.value} selected={v.budgetTier === option.value}>{option.label}</option>{/each}</select>
			</div>
			<div>
				<label class="label" for="setType">Slot</label>
				<select id="setType" name="setType" class="input"><option value="">Not set</option>{#each SET_TYPES as option (option.value)}<option value={option.value} selected={v.setType === option.value}>{option.label}</option>{/each}</select>
			</div>
		</div>
		<div><label class="label" for="genres">Sound of the night</label><GenreInput value={v.genres ?? ''} /></div>
		<div>
			<label class="label" for="message">Message</label>
			<textarea id="message" name="message" class="input min-h-36" required maxlength="2000" placeholder="Tell them about the night, the crowd, and what you'd like from them.">{v.message ?? ''}</textarea>
		</div>
		<div class="flex flex-col-reverse gap-3 sm:flex-row sm:justify-end">
			<a href="/projects/{data.project.slug}" class="btn-ghost">Cancel</a>
			<button class="btn-primary">Send inquiry</button>
		</div>
	</form>
</div>
