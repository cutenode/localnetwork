<script lang="ts">
	import SectionHeader from '#lib/ui/SectionHeader.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<div class="container-page flex flex-col gap-8 py-10 sm:gap-10 sm:py-14">
	<SectionHeader level={1} eyebrow="Privacy" title="What we store and where" lede="Where each kind of data lives, and who can read it." />
	<div class="prose prose-site">
		<h2>Public</h2>
		<p>Published project profiles (name, genres, city, links, mixes, fee tier, managers' DIDs) are public atproto records anyone can read.</p>
		<p>A bug report you send while signed in is a public post on userinput.app, kept in your own repo, with the page you sent it from and your browser.</p>
		<h2>Booking details</h2>
		<p>Private booking details (email, phone, rates, tech rider) are stored in a space only Local Network and its staff can read. The project's managers see them, and so does anyone they let into one of their press kits, inside that press kit.</p>
		<h2>Staff only</h2>
		<p>Requests, claim evidence, decisions, staff roles, and the audit log live in spaces only staff can read. Audit entries record what happened (who, what, when, and which fields changed); booking message contents are never copied into the audit log.</p>
		<h2>Press kits</h2>
		<p>A press kit's public part is a record in Local Network's repo that anyone can read. The rest sits in a space under Local Network's account that only the project's managers, Local Network, and the people they let in can read. Asking to see it shares your handle and any message you write with the project's managers. Each person let in gets a record in that space saying so, which any of the managers can have deleted to take access away. Drafts of a press kit sit in each manager's repo in a space only the project's current managers can read.</p>
		<p>When someone let in opens a press kit on this site, its project's managers see who and when. Local Network keeps that receipt for {data.retentionDays} days, and any of the managers can delete it sooner. The audit log keeps a record that the press kit was viewed, but not by whom.</p>
		<h2>Just you and the other party</h2>
		<p>Booking threads are readable only by the promoter and the project's managers. Your planner is readable only by you.</p>
		<h2>On our server</h2>
		<p>We keep a search index derived from the records above, OAuth tokens needed to act on your behalf (encrypted at rest by your host's disk encryption), hashed browser session tokens, and an outbox of audit entries waiting to be written. Signing out revokes our OAuth session.</p>
		<p>Your inbox lives only on our server. Each item says what happened and where (a new inquiry or message in one of your booking threads, say) but not what anyone wrote. Read items are deleted after 90 days and unread ones after 180.</p>
		<p>If you turn on email for your inbox, we keep the email address your account's host shares with us, to send it to, and delete it as soon as you turn email off. We send one email with a link to confirm the address is yours, and nothing else until you open it. Each email goes through our email provider, Resend, and has a link that turns email off without signing in.</p>
		<p>If you turn on Bluesky DMs for your inbox, they come from Local Network's own Bluesky account. Bluesky's chat service carries and keeps them like any other DM, and all we keep is that you turned them on.</p>
		<h2>Deleting data</h2>
		<p>Your requests, messages, and audit copies live in your own repo; you can delete them with any atproto tool. Ask an admin to archive a project you manage.</p>
	</div>
</div>
