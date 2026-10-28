<script lang="ts">
	import type { ShowFormValues } from '#lib/shows.ts';
	import Card from '#lib/ui/Card.svelte';
	import PlacePicker from './PlacePicker.svelte';

	/** The announce form: what, when and where on the venue's clock, and a ticket link. `editing` adds cancelling, and the versions its save swaps on. */
	let { values, geocoder = true, editing = false }: { values: ShowFormValues; geocoder?: boolean; editing?: boolean } = $props();

	const place = $derived({ city: values.city, region: values.region, country: values.country, latE6: values.latE6, lngE6: values.lngE6 });
</script>

<Card as="section" class="flex flex-col gap-4 sm:p-6">
	<h2 class="m-0 text-emphasis font-semibold">The show</h2>
	<div><label class="label" for="name">Show name</label><input id="name" name="name" class="input" required maxlength="120" value={values.name} /></div>
	<div class="grid gap-4 sm:grid-cols-3">
		<div><label class="label" for="date">Date</label><input id="date" type="date" name="date" class="input" required value={values.date} /></div>
		<div><label class="label" for="start">Starts</label><input id="start" type="time" name="start" class="input" required value={values.start} /></div>
		<div>
			<label class="label" for="end">Ends <span class="font-normal text-fg-subtle">(optional)</span></label>
			<input id="end" type="time" name="end" class="input" value={values.end} aria-describedby="end-hint" />
		</div>
	</div>
	<p id="end-hint" class="hint -mt-2">Times are the venue's. An end before the start is the next morning.</p>
	<div><label class="label" for="tickets">Ticket link <span class="font-normal text-fg-subtle">(optional)</span></label><input id="tickets" type="url" name="tickets" class="input" maxlength="2000" placeholder="https://" value={values.tickets} /></div>
	{#if editing}
		<input type="hidden" name="showCid" value={values.showCid} />
		<input type="hidden" name="eventCid" value={values.eventCid} />
		<label class="flex items-center gap-2.5 text-small text-fg-soft"><input type="checkbox" class="checkbox" name="cancelled" checked={values.cancelled} />This show is cancelled</label>
	{/if}
</Card>
<Card as="section" class="flex flex-col gap-4 sm:p-6">
	<h2 class="m-0 text-emphasis font-semibold">Where</h2>
	<div><label class="label" for="venue">Venue <span class="font-normal text-fg-subtle">(optional)</span></label><input id="venue" name="venue" class="input" maxlength="120" value={values.venue} /></div>
	<PlacePicker values={place} {geocoder} missing="Pick the city from the search, so the show's time is right." />
</Card>
