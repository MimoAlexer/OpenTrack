/** Restore a closed detail's row only when no surviving control owns focus. */
export function restoreIssueRowFocus(issueId: string) {
  const focusLost = () => !document.activeElement || document.activeElement === document.body || !document.activeElement.isConnected;
  if (!focusLost()) return;
  requestAnimationFrame(() => {
    if (focusLost()) document.getElementById(`row-${issueId}`)?.focus();
  });
}
