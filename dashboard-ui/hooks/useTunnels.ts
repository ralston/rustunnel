'use client';

import { useState, useCallback, useEffect } from 'react';
import type { ApiClient, Tunnel } from '@/lib/types';
import { useInterval } from './useInterval';

export function useTunnels(api: ApiClient, enabled: boolean) {
  const [tunnels, setTunnels] = useState<Tunnel[]>([]);
  const [error, setError] = useState<string | null>(null);

  const poll = useCallback(async () => {
    if (!enabled) return;
    try {
      const data = (await api.get('/api/tunnels')) as Tunnel[];
      setTunnels(data);
      setError(null);
    } catch (e) {
      setError((e as Error).message);
    }
  }, [api, enabled]);

  useEffect(() => { poll(); }, [poll]);
  useInterval(poll, 2000);

  return { tunnels, error, refresh: poll };
}
