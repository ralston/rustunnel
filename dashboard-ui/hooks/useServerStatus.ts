'use client';

import { useState, useCallback, useEffect } from 'react';
import type { ServerStatus } from '@/lib/types';
import { useInterval } from './useInterval';

export function useServerStatus(): ServerStatus | null {
  const [status, setStatus] = useState<ServerStatus | null>(null);

  const poll = useCallback(async () => {
    try {
      const s = await fetch('/api/status').then((r) => r.json());
      setStatus(s);
    } catch {
      setStatus(null);
    }
  }, []);

  useEffect(() => { poll(); }, [poll]);
  useInterval(poll, 2000);

  return status;
}
