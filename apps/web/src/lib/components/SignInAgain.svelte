<script lang="ts">
	/**
	 * For a write the session can't make, since it was signed in before Local Network asked for that permission:
	 * `error` says what signing in again allows. It posts straight to sign-in with the account's DID, so nobody types
	 * their handle again, and comes back to `next`.
	 */
	let { did, next, error }: { did: string; next: string; error: string } = $props();
</script>

<form method="POST" action="/login" class="notice notice-info m-0 flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
	<input type="hidden" name="handle" value={did} />
	<input type="hidden" name="next" value={next} />
	<p role="alert" class="m-0">{error}. It takes a moment and brings you back here.</p>
	<button class="btn-primary btn-sm">Sign in again</button>
</form>
