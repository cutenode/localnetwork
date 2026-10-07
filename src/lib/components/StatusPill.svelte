<script lang="ts">
	let { status }: { status: string } = $props();

	const waiting = 'bg-ansi-yellow text-ansi-black';

	const pushback = 'bg-ansi-brown text-ansi-white';

	const good = 'bg-ansi-green text-ansi-black';

	const bad = 'bg-ansi-red text-ansi-white';

	const inactive = 'bg-ansi-dark text-ansi-white';

	const info = 'bg-ansi-cyan text-ansi-black';

	const styles = new Map(
		Object.entries({
			pending: waiting,
			changes_requested: pushback,
			approved: good,
			rejected: bad,
			withdrawn: inactive,
			active: good,
			hidden: waiting,
			archived: inactive,
			open: info,
			negotiating: 'bg-ansi-magenta text-ansi-white',
			confirmed: good,
			declined: bad,
			cancelled: inactive,
			success: good,
			failure: bad,
			denied: pushback,
			written: info,
			verified: good,
			failed: bad,
			self: inactive,
			unsupported: inactive
		})
	);

	const labels = new Map(Object.entries({ changes_requested: 'changes requested', self: 'root is actor', unsupported: 'no spaces support' }));
</script>

<span class="pill {styles.get(status) ?? inactive}">{labels.get(status) ?? status}</span>
