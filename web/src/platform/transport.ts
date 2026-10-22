export type HttpTransport = (path: string, init: RequestInit) => Promise<Response>;
let selected: { transport: HttpTransport } | null = null;

export function sendRequest(path: string, init: RequestInit): Promise<Response> {
  return selected ? selected.transport(path, init) : fetch(`/api/v1${path}`, init);
}

// Native startup installs the bridge before any account/workspace is mounted.
// Releasing an older installation cannot remove its successor.
export function installTransport(transport: HttpTransport): () => void {
  const installation = { transport };
  selected = installation;
  return () => { if (selected === installation) selected = null; };
}
