// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { executeDriveBridgeRequest } from './drive-bridge.js';
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
});