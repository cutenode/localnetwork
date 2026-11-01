<script lang="ts">
	import type { ProjectDetails } from '#lib/types.ts';

	/**
	 * A project's private booking details: contacts, rates, the tech rider and hospitality. `details` is null for anyone
	 * who isn't a manager, staff, or reading one of the project's press kits with access, since every other look at them
	 * goes through a press kit, which records it. With nothing on file, a way to reach the managers instead.
	 */
	let { details, claimed, kitsHref }: { details: ProjectDetails | null; claimed: boolean; kitsHref?: string } = $props();

	const filled = $derived(details && Object.values(details).some(Boolean) ? details : null);
</script>

{#if !details}
	<p class="m-0 text-small text-fg-muted">Contacts, rates and the tech rider come with a press kit.{#if kitsHref}{' '}<a class="link" href={kitsHref}>See the press kits</a>{/if}</p>
{:else if filled}
	<dl class="m-0 grid gap-4 text-small sm:grid-cols-2">
		{#if filled.bookingEmail}<div><dt class="eyebrow text-fg-subtle">Booking email</dt><dd class="m-0 mt-1"><a class="link" href="mailto:{filled.bookingEmail}">{filled.bookingEmail}</a></dd></div>{/if}
		{#if filled.bookingPhone}<div><dt class="eyebrow text-fg-subtle">Phone</dt><dd class="m-0 mt-1">{filled.bookingPhone}</dd></div>{/if}
		{#if filled.rateNotes}<div class="sm:col-span-2"><dt class="eyebrow text-fg-subtle">Rates</dt><dd class="m-0 mt-1 whitespace-pre-line">{filled.rateNotes}</dd></div>{/if}
		{#if filled.techRider}<div class="sm:col-span-2"><dt class="eyebrow text-fg-subtle">Tech rider</dt><dd class="m-0 mt-1 whitespace-pre-line">{filled.techRider}</dd></div>{/if}
		{#if filled.hospitality}<div class="sm:col-span-2"><dt class="eyebrow text-fg-subtle">Hospitality</dt><dd class="m-0 mt-1 whitespace-pre-line">{filled.hospitality}</dd></div>{/if}
	</dl>
{:else}
	<p class="m-0 text-small text-fg-muted">No private booking details yet.{#if claimed}{" "}Send an inquiry to reach the managers directly.{/if}</p>
{/if}
