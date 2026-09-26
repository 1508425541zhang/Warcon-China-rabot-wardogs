/** Poll visible pages without overlapping requests or disturbing hidden tabs. */
export function refreshVisible(refresh: () => Promise<void>): () => void {
	let busy = false;
	const timer = setInterval(async () => {
		if (document.hidden || busy) return;
		busy = true;
		try {
			await refresh();
		} catch {
			/* Keep the last snapshot on a transient failure. */
		} finally {
			busy = false;
		}
	}, 10_000);
	return () => clearInterval(timer);
}
