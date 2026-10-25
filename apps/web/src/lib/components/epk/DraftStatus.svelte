<script lang="ts">
	import { DRAFT_STATUS_LABELS, type EpkDraftStatus } from '#lib/epk.ts';

	/** Where a draft stands against the published press kit. */
	let { status }: { status: EpkDraftStatus } = $props();

	// The same fills as StatusPill: each text colour stays above 4.5:1 on its own fill over any surface.
	const STYLES: Record<EpkDraftStatus, string> = {
		new: 'bg-link/15 text-link',
		current: 'bg-good/15 text-good',
		behind: 'bg-warning/15 text-warning',
		deleted: 'bg-critical/15 text-critical'
	};
</script>

<span class="pill {STYLES[status]}">{DRAFT_STATUS_LABELS[status]}</span>
