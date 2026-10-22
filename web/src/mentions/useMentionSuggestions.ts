import { useEffect, useState } from 'react';
import { request } from '../api';
import { useMentions } from './context';
import type { MentionPage } from './types';
export function useMentionSuggestions(query: string | null) {
  const context = useMentions(); const path = context ? `/workspaces/${context.workspaceId}/mention-candidates?${new URLSearchParams([...Object.entries(context.target).filter((entry): entry is [string, string] => typeof entry[1] === 'string'), ['q', query ?? ''], ['limit', '20']])}` : '';
  const key = `${context?.key}:${path}`; const [retry, setRetry] = useState(0);
  const [state, setState] = useState<{ key: string; page: MentionPage | null; loading: boolean; error: unknown }>({ key: '', page: null, loading: false, error: null });
  useEffect(() => {
    const abort = new AbortController(); setState({ key, page: null, loading: query !== null && !!path, error: null });
    if (query === null || !path) return () => abort.abort();
    const timer = setTimeout(() => { request<MentionPage>(path, { signal: abort.signal }).then(page => { if (!abort.signal.aborted) setState({ key, page, loading: false, error: null }); }).catch(error => { if (!abort.signal.aborted) setState({ key, page: null, loading: false, error }); }); }, query ? 150 : 0);
    return () => { clearTimeout(timer); abort.abort(); };
  }, [key, query === null, retry]);
  const current = state.key === key && query !== null;
  return { choices: current && !state.error ? state.page?.data ?? [] : [], loading: query !== null && (!current || state.loading), error: current ? state.error : null, hasMore: current && !!state.page?.has_more, refresh: () => setRetry(value => value + 1), key };
}
