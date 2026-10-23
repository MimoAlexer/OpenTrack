/** Restore a closed detail's row only when no surviving control owns focus. */
export function restoreIssueRowFocus(issueId: string, rowIds: readonly string[] = [], heading?: HTMLElement | null, current?: () => boolean) {
  const focusLost = () => !document.activeElement || document.activeElement === document.body || !document.activeElement.isConnected;
  if (current?.() === false || !focusLost()) return;
  requestAnimationFrame(() => {
    if (current?.() === false || !focusLost()) return;
    let row = document.getElementById(`row-${issueId}`);
    const origin = rowIds.indexOf(`row-${issueId}`);
    for (let distance = 1; !row && origin >= 0 && distance < rowIds.length; distance++) {
      row = document.getElementById(rowIds[origin + distance] ?? '') ?? document.getElementById(rowIds[origin - distance] ?? '');
    }
    (row ?? document.querySelector<HTMLElement>('.issue-groups a.issue-row') ?? heading)?.focus();
  });
}
