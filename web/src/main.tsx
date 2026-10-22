// Declare cascade layers before component modules import their styles.
import './styles.css';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { App } from './App';
import { DesktopProvider } from './desktop/DesktopProvider';
import { DesktopStore } from './desktop/store';
import './desktop/desktop.css';

const app = isTauri() ? <DesktopProvider store={new DesktopStore(invoke, listen)}><App /></DesktopProvider> : <App />;
createRoot(document.getElementById('root')!).render(<StrictMode>{app}</StrictMode>);
