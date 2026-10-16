<script lang="ts">
	let { status }: { status: string } = $props();

	// Subtle fills by meaning; each text colour stays above 4.5:1 on its own fill over any surface.
	const good = 'bg-good/15 text-good';

	const warning = 'bg-warning/15 text-warning';

	const critical = 'bg-critical/15 text-critical';

	const info = 'bg-link/15 text-link';

	const neutral = 'bg-well text-fg-muted';

	const styles = new Map(
		Object.entries({
			approved: good,
			active: good,
			confirmed: good,
			success: good,
			verified: good,
			pending: warning,
			hidden: warning,
			negotiating: warning,
			changes_requested: warning,
			denied: warning,
			rejected: critical,
			declined: critical,
			failure: critical,
			failed: critical,
			open: info,
			written: info
		})
	);

	const labels = new Map(Object.entries({ changes_requested: 'Changes requested', self: 'Root is actor', unsupported: 'No spaces support' }));

	const label = $derived(labels.get(status) ?? status.charAt(0).toUpperCase() + status.slice(1));
</script>

<span class="pill {styles.get(status) ?? neutral}">{label}</span>
