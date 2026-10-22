import { useEffect, useState } from 'react';
import { request } from '../api';

export function useResource<T>(path: string | null) {
  const [data, setData] = useState<T | null>(null);
  const [dataPath, setDataPath] = useState(path);
  const [loading, setLoading] = useState(!!path);
  const [error, setError] = useState<unknown>(null);
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    const abort = new AbortController(); let active = true;
    setData(null); setDataPath(path); setError(null); setLoading(!!path);
    if (path) request<T>(path, { signal: abort.signal }).then(value => {
      if (active) setData(value);
    }).catch(error => { if (active && !abort.signal.aborted) setError(error); }).finally(() => { if (active) setLoading(false); });
    return () => { active = false; abort.abort(); };
  }, [path, revision]);
  return { data: dataPath === path ? data : null, loading: dataPath !== path || loading, error: dataPath === path ? error : null, reload: () => setRevision(value => value + 1) };
}
