<script lang="ts">
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
</script>

<svelte:head><title>Privacy · Local Network</title></svelte:head>

<div class="container-page flex flex-col gap-8 py-10 sm:gap-10 sm:py-14">
	<SectionHeader level={1} eyebrow="Privacy" title="What we store and where" lede="Where each kind of data lives, and who can read it." />
	<div class="prose prose-site">
		<h2>Public</h2>
		<p>Published project profiles (name, genres, city, links, mixes, fee tier, managers' DIDs) are public atproto records anyone can read.</p>
		<h2>Members only</h2>
		<p>Private booking details (email, phone, rates, tech rider) are stored in the industry space and shown only to signed-in members.</p>
		<h2>Staff only</h2>
		<p>Requests, claim evidence, decisions, staff roles, and the audit log live in spaces only staff can read. Audit entries record what happened (who, what, when, and which fields changed); booking message contents are never copied into the audit log.</p>
		<h2>Press kits</h2>
		<p>A press kit's public part is a record in its owner's repo that anyone can read. The rest sits in a space under the owner's account that only they, Local Network, and the people they let in can read. Asking to see it shares your handle and any message you write with the owner. Each person let in gets a record in that space saying so, which the owner can delete to take access away.</p>
		<h2>Just you and the other party</h2>
		<p>Booking threads are readable only by the promoter and the project's managers. Your planner is readable only by you.</p>
		<h2>On our server</h2>
		<p>We keep a search index derived from the records above, OAuth tokens needed to act on your behalf (encrypted at rest by your host's disk encryption), hashed browser session tokens, and an outbox of audit entries waiting to be written. Signing out revokes our OAuth session.</p>
		<h2>Deleting data</h2>
		<p>Your requests, messages, and audit copies live in your own repo; you can delete them with any atproto tool. Ask an admin to archive a project you manage.</p>
	</div>
</div>
