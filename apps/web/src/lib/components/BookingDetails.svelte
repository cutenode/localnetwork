<script lang="ts">
	import type { ProjectDetails } from '#lib/types.ts';

	/**
	 * A project's private booking details, for signed-in members: contacts, rates, the tech rider
	 * and hospitality. Signed out, a prompt to sign in; with nothing on file, a way to reach the
	 * managers instead.
	 */
	let { details, signedIn, claimed, loginHref }: { details: ProjectDetails | null; signedIn: boolean; claimed: boolean; loginHref: string } = $props();

	const filled = $derived(details && Object.values(details).some(Boolean) ? details : null);
</script>

{#if !signedIn}
	<p class="m-0 text-small text-fg-muted"><a class="link" href={loginHref}>Sign in</a> to see booking contacts, rates and the tech rider.</p>
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
