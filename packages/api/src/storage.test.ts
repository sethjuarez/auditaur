// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { executeDriveBridgeRequest } from './drive-bridge.js';
import { UNLOAD_CLEAR_WINDOW_MS } from './storage.js';
import type { DriveBridgeRequest } from './types.js';

// Node's built-in localStorage shadows happy-dom's, so give each store a plain in-memory Storage.
function memoryStorage(): Storage {
  const data = new Map<string, string>();
  return {
    get length() {
      return data.size;
    },
    clear: () => data.clear(),
    getItem: (key) => data.get(key) ?? null,
    key: (index) => Array.from(data.keys())[index] ?? null,
    removeItem: (key) => void data.delete(key),
    setItem: (key, value) => void data.set(key, String(value)),
  };
}

beforeEach(() => {
  vi.stubGlobal('localStorage', memoryStorage());
  vi.stubGlobal('sessionStorage', memoryStorage());
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('clearStorage', () => {
  it('clears local and session storage and cookies and reports counts', async () => {
    localStorage.setItem('cutready:layout', 'focus');
    localStorage.setItem('other', '1');
    sessionStorage.setItem('tab', 'x');
    document.cookie = 'theme=dark; path=/';

    const result = await executeDriveBridgeRequest({
      schemaVersion: 1,
      protocolVersion: 1,
      requestId: 'r1',
      action: 'clearStorage',
      visibleOnly: false,
      createdAtUnixNanos: 0,
    } as DriveBridgeRequest);

    expect(result).toMatchObject({ ok: true, localStorage: 2, sessionStorage: 1, cookies: 1 });
    expect(localStorage.length).toBe(0);
    expect(sessionStorage.length).toBe(0);
    expect(document.cookie).not.toContain('theme=dark');
  });

  const request = {
    schemaVersion: 1,
    protocolVersion: 1,
    requestId: 'r2',
    action: 'clearStorage',
    visibleOnly: false,
    createdAtUnixNanos: 0,
  } as DriveBridgeRequest;

  it('clears again after the app saves state while the page unloads', async () => {
    const saveLayout = () => localStorage.setItem('cutready:layout', 'focus');
    window.addEventListener('pagehide', saveLayout);
    try {
      await executeDriveBridgeRequest(request);
      window.dispatchEvent(new Event('pagehide'));
      expect(localStorage.getItem('cutready:layout')).toBeNull();
    } finally {
      window.removeEventListener('pagehide', saveLayout);
    }
  });

  it('ignores visibilitychange while the page is still visible', async () => {
    await executeDriveBridgeRequest(request);
    localStorage.setItem('kept', '1');
    document.dispatchEvent(new Event('visibilitychange'));
    window.dispatchEvent(new Event('visibilitychange'));
    expect(localStorage.getItem('kept')).toBe('1');
  });

  it('stops clearing on unload once the reload window passes', async () => {
    vi.useFakeTimers();
    try {
      await executeDriveBridgeRequest(request);
      vi.advanceTimersByTime(UNLOAD_CLEAR_WINDOW_MS + 1);
      localStorage.setItem('later', '1');
      window.dispatchEvent(new Event('pagehide'));
      expect(localStorage.getItem('later')).toBe('1');
    } finally {
      vi.useRealTimers();
    }
  });
});