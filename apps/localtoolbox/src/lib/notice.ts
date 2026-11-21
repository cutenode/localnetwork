/**
 * Set when the extension is first installed (background.ts) and taken away
 * when the popup's first-run notice is read (FirstRun.svelte). While it is
 * there, content.ts leaves SoundCloud's download buttons as they are, so no
 * gate is walked and no address handed on before the person has read where it
 * goes. content.ts names it too, held in step by test/consistency.test.ts.
 */
export const NOTICE_PENDING = 'noticePending';
