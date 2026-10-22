import { importMarkdownDocument } from './portability-markdown';

const scope = globalThis as unknown as { onmessage: ((event: MessageEvent<unknown>) => void) | null; postMessage: (message: unknown) => void };
scope.onmessage = event => {
  try {
    if (typeof event.data !== 'string') throw new Error('Markdown input must be text.');
    scope.postMessage({ ok: true, result: importMarkdownDocument(event.data) });
  } catch (error) {
    scope.postMessage({ ok: false, error: error instanceof Error ? error.message : 'Markdown could not be imported.' });
  }
};
