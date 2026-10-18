import { useEffect, useRef, useState } from 'react';

export function pushRoute<T>(history: T[], route: T): T[] {
  if (JSON.stringify(history.at(-1)) === JSON.stringify(route)) return history;
  return [...history, route].slice(-50);
}
/** In-app history, never window.history.back(): the latter could leave the desktop application. */
export function useNavigationHistory<T>(route: T, scope: string | null, restore: (route: T) => Promise<void>, onError: (error: unknown) => void) {
  const history = useRef<T[]>([]);
  const scopeRef = useRef(scope);
  const restoring = useRef(false);
  const [count, setCount] = useState(0);
  const [busy, setBusy] = useState(false);
  const key = JSON.stringify(route);
  useEffect(() => {
    if (scopeRef.current !== scope) { history.current = []; scopeRef.current = scope; }
    if (!restoring.current) history.current = pushRoute(history.current, JSON.parse(key) as T);
    setCount(history.current.length);
  }, [key, scope]);
  return { canGoBack: count > 1 && !busy, goBack: async () => {
    if (restoring.current || history.current.length < 2) return;
    const expectedScope = scopeRef.current;
    const previous = history.current[history.current.length - 2]!;
    restoring.current = true; setBusy(true);
    try {
      await restore(previous);
      if (scopeRef.current === expectedScope) { history.current.pop(); setCount(history.current.length); }
    } catch (error) { onError(error); }
    finally { restoring.current = false; setBusy(false); }
  } };
}
