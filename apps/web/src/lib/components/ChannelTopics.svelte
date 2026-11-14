<script module lang="ts">
	import type { ChannelView, NotificationChannel, NotificationTopic } from '#lib/types.ts';

	/** Each topic's name, and what it covers in a few words. */
	const TOPICS: Record<NotificationTopic, { label: string; hint: string }> = {
		bookings: { label: 'Bookings', hint: 'Inquiries, messages, status' },
		guests: { label: 'Guests', hint: 'Guest list changes' },
		waitlist: { label: 'Waitlist', hint: 'Spots opening up' },
		events: { label: 'Events', hint: 'New events' }
	};

	const NAMES: Record<NotificationChannel, string> = { email: 'Email', dm: 'Bluesky DM' };

	/** One channel's column: its setting, null while it's off. */
	export type ChannelColumn = { channel: NotificationChannel; view: ChannelView | null };
</script>

<script lang="ts">
	import { enhance } from '$app/forms';
	import { formFields } from '#lib/form-fields.svelte.ts';

	/**
	 * Which topics go where: a row per topic, a column for the site (which always gets everything) and one per channel
	 * the site can send by. A ticked box saves its channel's column on the spot, posting the checked topics as `topic`
	 * to `?/emailTopics` or `?/dmTopics`; without JavaScript, a Save button per channel does.
	 */
	let { topics, channels }: { topics: NotificationTopic[]; channels: ChannelColumn[] } = $props();

	const id = $props.id();

	// Bound, so boxes ticked before the page hydrated stay.
	const checked = $derived(formFields(Object.fromEntries(channels.map(({ channel, view }) => [channel, view?.topics ?? []]))));

	/** What the last save said, read out once it's done. */
	let saved = $state('');

	const STATUS = { active: 'On', paused: 'Paused' } as const;
</script>

{#each channels as { channel } (channel)}
	<form
		id="{id}-{channel}"
		method="POST"
		action="?/{channel}Topics"
		hidden
		use:enhance={() =>
			async ({ result, update }) => {
				// A save says so here rather than on the card below, and the boxes already show it. Anything else (signed
				// out, a server error) goes the usual way.
				if (result.type !== 'success') return update({ reset: false });
				saved = String(result.data?.success ?? 'Saved.');
			}}
	></form>
{/each}

<section aria-labelledby="{id}-heading" class="flex flex-col gap-3">
	<!-- The save says so beside the heading, so nothing below moves when it does. -->
	<div class="flex items-baseline justify-between gap-4">
		<h2 id="{id}-heading" class="m-0 text-emphasis font-semibold">What goes where</h2>
		<p class="m-0 text-small text-fg-muted" role="status">{saved}</p>
	</div>
	<div class="card overflow-hidden p-0">
		<table class="w-full border-collapse text-left">
			<caption class="sr-only">Where each kind of news goes</caption>
			<thead>
				<tr class="border-b border-line">
					<th scope="col" class="px-4 py-3 text-label font-medium text-fg-muted sm:px-5"><span class="sr-only">Topic</span></th>
					<th scope="col" class="w-16 px-2 py-3 text-center text-label font-medium text-fg sm:w-24">Site</th>
					{#each channels as { channel, view } (channel)}
						<th scope="col" class="w-20 px-2 py-3 text-center text-label font-medium text-fg sm:w-28">
							<span class="flex flex-col items-center gap-0.5">
								{NAMES[channel]}
								<span class="flex items-center gap-1 text-caption font-normal text-fg-muted">
									<span class={['size-1.5 rounded-full', !view ? 'bg-line-strong' : view.status === 'active' ? 'bg-good' : 'bg-warning']} aria-hidden="true"></span>
									{view ? STATUS[view.status] : 'Off'}
								</span>
							</span>
						</th>
					{/each}
				</tr>
			</thead>
			<tbody class="divide-y divide-line">
				{#each topics as topic (topic)}
					<tr>
						<th scope="row" class="px-4 py-3 font-normal sm:px-5">
							<span class="flex flex-col"><span class="text-small font-medium text-fg">{TOPICS[topic].label}</span><span class="text-caption text-fg-muted">{TOPICS[topic].hint}</span></span>
						</th>
						<td class="px-2 py-3 text-center"><input type="checkbox" class="checkbox" checked disabled aria-label="{TOPICS[topic].label} on the site, always" /></td>
						{#each channels as { channel, view } (channel)}
							<td class="px-2 py-3 text-center">
								<input
									type="checkbox"
									class="checkbox"
									form="{id}-{channel}"
									name="topic"
									value={topic}
									disabled={!view}
									aria-label="{TOPICS[topic].label} by {NAMES[channel]}"
									bind:group={checked[channel]}
									onchange={(event) => event.currentTarget.form?.requestSubmit()}
								/>
							</td>
						{/each}
					</tr>
				{/each}
			</tbody>
		</table>
		<noscript>
			<div class="flex flex-wrap gap-3 border-t border-line px-4 py-3 sm:px-5">
				{#each channels as { channel, view } (channel)}
					{#if view}<button type="submit" form="{id}-{channel}" class="btn-secondary btn-sm">Save {NAMES[channel]}</button>{/if}
				{/each}
			</div>
		</noscript>
	</div>
</section>
