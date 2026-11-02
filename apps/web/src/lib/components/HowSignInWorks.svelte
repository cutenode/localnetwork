<script lang="ts">
	import { Check, House, KeyRound, Ticket, User, X } from '@lucide/svelte';
	import { CAN, CANNOT } from '#lib/sign-in.ts';
	import Mark from '#lib/ui/Mark.svelte';

	/**
	 * "How signing in works": a modal dialog any button opens with `commandfor={id} command="show-modal"`, no script
	 * needed. What it says Local Network can do comes from the scopes it asks for (#lib/sign-in.ts).
	 */
	let { id }: { id: string } = $props();

	const steps = [
		{ label: 'You', icon: User },
		{ label: 'Your host', icon: House },
		{ label: 'Local Network', icon: null }
	];
</script>

<dialog {id} aria-labelledby="{id}-title" closedby="any" class="m-auto w-[min(34rem,calc(100vw-2rem))] max-w-none rounded-xl border border-line-strong bg-surface p-0 text-fg backdrop:bg-canvas/80">
	<div class="flex items-center justify-between gap-4 border-b border-line px-5 py-4 sm:px-6">
		<h2 id="{id}-title" class="m-0 text-emphasis font-semibold">How signing in works</h2>
		<button type="button" class="btn-ghost btn-sm -mr-2 px-2" commandfor={id} command="close" aria-label="Close"><X class="size-4" /></button>
	</div>
	<div class="flex flex-col gap-5 px-5 py-5 sm:px-6">
		<p class="m-0 text-fg-soft">It works like signing in with Google, with the host that keeps your account in Google's place.</p>
		<ol class="m-0 flex list-none items-start justify-between p-0" aria-label="Who sees what">
			{#each steps as step, i (step.label)}
				{#if i > 0}
					<li aria-hidden="true" class="mt-3.5 flex flex-1 items-center text-fg-subtle">
						<span class="h-px flex-1 bg-line-strong"></span>
						{#if i === 1}<KeyRound class="mx-1.5 size-3.5" />{:else}<Ticket class="mx-1.5 size-3.5" />{/if}
						<span class="h-px flex-1 bg-line-strong"></span>
					</li>
				{/if}
				<li class="flex w-20 flex-col items-center gap-2 text-center text-label text-fg-soft">
					<span class="grid size-10 place-items-center rounded-lg border border-line-strong bg-raised">
						{#if step.icon}<step.icon class="size-4.5" />{:else}<Mark class="h-4" />{/if}
					</span>
					{step.label}
				</li>
			{/each}
		</ol>
		<p class="m-0 text-small text-fg-muted">Your password only goes to your host, on its own page. Local Network gets a pass you approve.</p>
		<div class="grid gap-5 border-t border-line pt-5 sm:grid-cols-2">
			<div>
				<h3 class="m-0 mb-2 text-small font-semibold text-fg-muted">Local Network can</h3>
				<ul class="m-0 flex list-none flex-col gap-2 p-0 text-small">
					{#each CAN as line (line.text)}<li class="flex gap-2"><Check aria-hidden="true" class="mt-0.5 size-4 shrink-0 text-good" />{line.text}</li>{/each}
				</ul>
			</div>
			<div>
				<h3 class="m-0 mb-2 text-small font-semibold text-fg-muted">It can't</h3>
				<ul class="m-0 flex list-none flex-col gap-2 p-0 text-small">
					{#each CANNOT as text (text)}<li class="flex gap-2"><X aria-hidden="true" class="mt-0.5 size-4 shrink-0 text-fg-subtle" />{text}</li>{/each}
				</ul>
			</div>
		</div>
		<p class="m-0 border-t border-line pt-5 text-small text-fg-muted">
			Local Network keeps the pass on its server so you stay signed in, and signing out ends it. If your host can't hold private records yet, Local Network keeps them for you. Everything else stays in your account at your host.
		</p>
	</div>
</dialog>
