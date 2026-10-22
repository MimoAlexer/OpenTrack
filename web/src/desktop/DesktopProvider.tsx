import { Fragment, useEffect, useSyncExternalStore, type ReactNode } from 'react';
import { Brand, ErrorBanner, Loading } from '../components';
import { installAttachmentBridge } from './bridge';
import { installTransport } from '../platform/transport';
import { ConnectionScreen } from './ConnectionScreen';
import { DesktopContext } from './context';
import { ConfirmActionProvider } from './ConfirmAction';
import { DesktopFrame } from './DesktopFrame';
import type { DesktopStore } from './store';

export function DesktopProvider({ store, children }: { store: DesktopStore; children: ReactNode }) {
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot);
  useEffect(() => {
    const release = installTransport(store.http);
    const releaseAttachments = installAttachmentBridge(store.bridge);
    void store.start();
    return () => { store.stop(); release(); releaseAttachments(); };
  }, [store]);
  if (snapshot.loading) return <main className="startup"><Brand /><Loading label="Opening desktop connection…" /></main>;
  if (!snapshot.connection) return <main className="startup"><Brand /><ErrorBanner error={snapshot.error} retry={() => void store.start()} /></main>;
  return <DesktopContext.Provider value={{ store, snapshot }}><ConfirmActionProvider><DesktopFrame>
    {snapshot.connection.selected ? <Fragment key={snapshot.connection.scope_generation}>{children}</Fragment> : <ConnectionScreen connect={(origin, http) => store.select(origin, http)} />}
  </DesktopFrame></ConfirmActionProvider></DesktopContext.Provider>;
}
