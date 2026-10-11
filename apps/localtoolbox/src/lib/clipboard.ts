// Copying text and images out of the popup.
//
// The async clipboard needs the document to be focused, which a popup loses
// easily (a file dialog, a click on the page behind it), so text falls back to
// the old selection route rather than failing silently.

/** @returns whether it was copied */
export async function copyText(text: string) {
	try {
		await navigator.clipboard.writeText(text);

		return true;
	} catch (_e) {
		return copyBySelection(text);
	}
}

function copyBySelection(text: string) {
	try {
		const area = document.createElement('textarea');
		area.value = text;
		area.setAttribute('aria-hidden', 'true');
		area.style.position = 'fixed';
		area.style.opacity = '0';
		document.body.append(area);
		area.select();
		const copied = document.execCommand('copy');
		area.remove();

		return copied;
	} catch (_e) {
		return false;
	}
}

/**
 * Fetches one image and puts it on the clipboard.
 *
 * This is the only time the extension fetches artwork, and it happens once
 * per click. Chrome's async clipboard accepts only PNG images, so a JPEG is
 * re-encoded from its decoded pixels.
 *
 * @returns whether it was copied
 */
export async function copyImage(url: string) {
	try {
		const response = await fetch(url);

		if (!response.ok) throw new Error(`HTTP ${response.status}`);
		const bitmap = await createImageBitmap(await response.blob());
		const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
		canvas.getContext('2d')!.drawImage(bitmap, 0, 0);
		const png = await canvas.convertToBlob({ type: 'image/png' });
		await navigator.clipboard.write([new ClipboardItem({ 'image/png': png })]);

		return true;
	} catch (_e) {
		return false;
	}
}

/**
 * Hands `text` to the browser as a file to save. Extension pages may download
 * through a link without any extra permission, which keeps the manifest as it
 * is.
 *
 * @returns the filename, as given
 */
export function downloadText(text: string, filename: string, mime = 'text/plain') {
	const url = URL.createObjectURL(new Blob([text], { type: mime }));
	const link = document.createElement('a');
	link.href = url;
	link.download = filename;
	link.click();
	setTimeout(() => URL.revokeObjectURL(url), 1000);

	return filename;
}
