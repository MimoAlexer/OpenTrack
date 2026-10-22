import type { Session } from '../types';
import { createNativeBridge, type NativeBridge } from './bridge';
import { createNativeHttp, NativeBridgeError, nativeError } from './http';
import type { ConnectionState, NativeAuthEvent, NativeInvoke, NativeIssueLink, NativeListen } from './types';

export interface DesktopSnapshot {
  connection: ConnectionState | null;
  session: Session | null;
  authEvent: NativeAuthEvent | null;
  issueLink: NativeIssueLink | null;
  loading: boolean;
  error: unknown;
}
function revision(value: string): bigint {
  if (!/^(0|[1-9][0-9]{0,19})$/.test(value)) throw new NativeBridgeError('native_state_invalid', 'The desktop returned an invalid connection state.', false);
  return BigInt(value);
}

export class DesktopStore {
  readonly bridge: NativeBridge;
  readonly http: ReturnType<typeof createNativeHttp>;
  private snapshot: DesktopSnapshot = { connection: null, session: null, authEvent: null, issueLink: null, loading: true, error: null };
  private listeners = new Set<() => void>();
  private stops: (() => void)[] = [];
  private bufferedAuth: NativeAuthEvent[] = [];
  private active = false;
  private lifecycle = 0;
  private linkEvents = 0;
  private authEvents = 0;

  constructor(invoke: NativeInvoke, listen: NativeListen) {
    const generation = () => this.snapshot.connection?.generation ?? '0';
    this.bridge = createNativeBridge(invoke, listen, generation);
    this.http = createNativeHttp(invoke, generation, () => this.snapshot.connection?.scope_generation ?? '0');
  }
  getSnapshot = (): DesktopSnapshot => this.snapshot;
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => this.listeners.delete(listener); };
  private publish(change: Partial<DesktopSnapshot>) {
    this.snapshot = { ...this.snapshot, ...change };
    this.listeners.forEach(listener => listener());
  }
  acceptConnection = (connection: ConnectionState) => {
    const previous = this.snapshot.connection;
    revision(connection.scope_generation); revision(connection.generation);
    if (previous && (revision(connection.generation) < revision(previous.generation) || revision(connection.scope_generation) < revision(previous.scope_generation))) return false;
    const changedScope = previous && connection.scope_generation !== previous.scope_generation;
    this.publish({ connection, ...(changedScope || !connection.credential_present ? { session: null } : {}), ...(changedScope ? { authEvent: null } : {}), loading: false, error: null });
    return true;
  };
  private acceptAuth = (event: NativeAuthEvent) => {
    if (!this.active) return;
    if (!this.snapshot.connection) { this.bufferedAuth.push(event); return; }
    try {
      const connection = this.snapshot.connection;
      if (revision(event.generation) < revision(connection.generation) || revision(event.scope_generation) < revision(connection.scope_generation)) return;
      this.authEvents += 1;
      const changedScope = event.scope_generation !== connection.scope_generation;
      const signedOut = event.kind === 'signed_out';
      const authenticated = event.kind === 'authenticated';
      if (authenticated && !event.session) throw new NativeBridgeError('native_state_invalid', 'The desktop sign-in did not return account information.', false);
      const nextConnection = { ...connection, generation: event.generation, scope_generation: event.scope_generation, credential_present: authenticated ? true : signedOut ? false : connection.credential_present, pending: event.kind === 'pending' ? connection.pending : null };
      this.publish({
        connection: nextConnection,
        session: authenticated ? event.session : signedOut || changedScope ? null : this.snapshot.session,
        authEvent: event,
        error: event.error ? new NativeBridgeError(event.error.code, event.error.message, event.error.retryable) : null,
      });
      if (event.kind === 'pending') void this.refresh().catch(error => { if (this.active) this.publish({ error }); });
    } catch (error) { this.publish({ error }); }
  };
  async start() {
    const lifecycle = ++this.lifecycle;
    this.active = true; this.publish({ loading: true, error: null });
    const stops: (() => void)[] = [];
    try {
      stops.push(await this.bridge.onAuth(event => { if (this.active && lifecycle === this.lifecycle) this.acceptAuth(event); }));
      stops.push(await this.bridge.onIssueLink(issueLink => { if (this.active && lifecycle === this.lifecycle) { this.linkEvents += 1; this.publish({ issueLink }); } }));
      if (!this.active || lifecycle !== this.lifecycle) { stops.forEach(stop => stop()); return; }
      this.stops.push(...stops);
      const linkRevision = this.linkEvents;
      const [connection, issueLink] = await Promise.all([this.bridge.read(), this.bridge.readIssueLink()]);
      if (!this.active || lifecycle !== this.lifecycle) return;
      this.acceptConnection(connection);
      if (linkRevision === this.linkEvents) this.publish({ issueLink });
      const buffered = this.bufferedAuth; this.bufferedAuth = [];
      buffered.forEach(this.acceptAuth);
    } catch (error) {
      stops.forEach(stop => stop()); this.stops = this.stops.filter(stop => !stops.includes(stop));
      if (this.active && lifecycle === this.lifecycle) this.publish({ loading: false, error: nativeError(error) });
    }
  }
  stop() { this.active = false; this.lifecycle += 1; this.stops.splice(0).forEach(stop => stop()); this.bufferedAuth = []; }
  async refresh() {
    const lifecycle = this.lifecycle;
    const authEvents = this.authEvents;
    const connection = await this.bridge.read();
    // Pending/failed transitions need not rotate credentials. A same-generation
    // read begun before a newer event must not restore an ended pending prompt.
    if (this.active && lifecycle === this.lifecycle && (authEvents === this.authEvents || revision(connection.generation) > revision(this.snapshot.connection?.generation ?? '0'))) this.acceptConnection(connection);
  }
  async begin(deviceName: string) {
    const receipt = await this.bridge.begin(deviceName);
    const connection = this.snapshot.connection;
    if (!this.active || !connection || receipt.generation !== connection.generation) return;
    const event = this.snapshot.authEvent;
    if (event?.request_id === receipt.request_id && event.kind !== 'pending') return;
    this.publish({ connection: { ...connection, pending: { request_id: receipt.request_id, confirmation_code: receipt.confirmation_code, expires_at: receipt.expires_at } }, error: null });
  }
  async cancel() { const connection = await this.bridge.cancel(); if (this.active) this.acceptConnection(connection); }
  async select(origin: string, developmentHttp: boolean) { const connection = await this.bridge.select(origin, developmentHttp); if (this.active) this.acceptConnection(connection); }
  async forget() { const connection = await this.bridge.forget(); if (this.active) this.acceptConnection(connection); }
  async logout() {
    const result = await this.bridge.logout();
    if (this.active) {
      const previous = this.snapshot.authEvent;
      if (this.acceptConnection(result)) {
        // The safe receipt can arrive before the native event. Preserve the
        // remote-result notice through the scope remount in either order.
        this.publish({ authEvent: previous?.kind === 'signed_out' && previous.generation === result.generation ? previous : { generation: result.generation, scope_generation: result.scope_generation, kind: 'signed_out', request_id: null, session: null, error: null, remote_revocation_confirmed: result.remote_revocation_confirmed } });
      }
    }
    return result;
  }
  async dismissIssueLink(id: string) {
    await this.bridge.dismissIssueLink(id);
    if (this.active && this.snapshot.issueLink?.id === id) this.publish({ issueLink: null });
  }
}
