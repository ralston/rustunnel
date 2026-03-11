'use client';

import type { ServerStatus } from '@/lib/types';
import { Dot } from './ui/Dot';

interface HeaderProps {
  status: ServerStatus | null;
  onSignOut: () => void;
}

export function Header({ status, onSignOut }: HeaderProps) {
  const ok = status?.ok === true;
  return (
    <header
      style={{
        display: 'flex',
        alignItems: 'center',
        padding: '0 20px',
        height: 50,
        background: 'var(--surface)',
        borderBottom: '1px solid var(--border)',
        gap: 12,
        position: 'sticky',
        top: 0,
        zIndex: 100,
      }}
    >
      <span style={{ fontSize: 18 }}>🔗</span>
      <span style={{ fontWeight: 600, fontSize: 14, letterSpacing: '-0.2px', marginRight: 'auto' }}>
        Rustunnel Dashboard
      </span>

      {status && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 14, color: 'var(--muted)', fontSize: 12 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
            <Dot color={ok ? 'var(--green)' : 'var(--red)'} pulse={ok} />
            <span style={{ color: ok ? 'var(--green)' : 'var(--red)' }}>
              {ok ? 'Healthy' : 'Offline'}
            </span>
          </div>
          <span title="Active sessions">
            {status.active_sessions} session{status.active_sessions !== 1 ? 's' : ''}
          </span>
          <span title="Active tunnels">
            {status.active_tunnels} tunnel{status.active_tunnels !== 1 ? 's' : ''}
          </span>
        </div>
      )}

      <button onClick={onSignOut} style={{ marginLeft: 8 }}>
        Sign Out
      </button>
    </header>
  );
}
