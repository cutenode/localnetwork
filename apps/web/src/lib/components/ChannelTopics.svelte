<script module lang="ts">
	import type { NotificationTopic } from '#lib/types.ts';

	/** What each topic covers, as a checkbox label. */
	const LABELS: Record<NotificationTopic, string> = {
		bookings: 'Booking inquiries, messages and status changes',
		guests: 'Guests',
		waitlist: 'Waitlist spots',
		events: 'New events'
	};
</script>

<script lang="ts">
	import { enhance } from '$app/forms';

	/** Picks which of `topics` a channel sends, posting the checked ones as `topic` to `action`. */
	let { action, legend, topics, selected }: { action: string; legend: string; topics: NotificationTopic[]; selected: NotificationTopic[] } = $props();
</script>

<form method="POST" {action} use:enhance={() => async ({ update }) => update({ reset: false })} class="flex flex-col items-start gap-3">
	<fieldset class="m-0 flex flex-col gap-2.5 border-0 p-0">
		<legend class="mb-2.5 p-0 text-small font-medium text-fg">{legend}</legend>
		{#each topics as topic (topic)}
			<label class="flex items-center gap-2.5 text-small text-fg-soft"><input type="checkbox" class="checkbox" name="topic" value={topic} checked={selected.includes(topic)} />{LABELS[topic]}</label>
		{/each}
	</fieldset>
	<button type="submit" class="btn-secondary btn-sm">Save</button>
</form>
