/**
 * Clears the page's web storage for a drill `clearStorage` step. The CLI reloads the page right
 * after, so app state rebuilt from storage starts fresh. IndexedDB deletes that are blocked by the
 * app's open connections finish once the reload closes them.
 */
export interface ClearStorageResult {
  localStorage: number;
  sessionStorage: number;
  indexedDb: string[];
  blockedIndexedDb: string[];
  caches: number;
  cookies: number;
}

export async function clearWebStorage(): Promise<ClearStorageResult> {
  const result: ClearStorageResult = {
    localStorage: clearStore(globalThis.localStorage),
    sessionStorage: clearStore(globalThis.sessionStorage),
    indexedDb: [],
    blockedIndexedDb: [],
    caches: 0,
    cookies: clearCookies(),
  };
  const idb = globalThis.indexedDB;
  if (idb && typeof idb.databases === 'function') {
    for (const { name } of await idb.databases()) {
      if (!name) {
        continue;
      }
      const outcome = await deleteDatabase(idb, name);
      (outcome === 'blocked' ? result.blockedIndexedDb : result.indexedDb).push(name);
    }
  }
  const cacheStorage = globalThis.caches;
  if (cacheStorage) {
    for (const key of await cacheStorage.keys()) {
      if (await cacheStorage.delete(key)) {
        result.caches += 1;
      }
    }
  }
  return result;
}

function clearStore(store: Storage | undefined): number {
  if (!store) {
    return 0;
  }
  const count = store.length;
  store.clear();
  return count;
}

function clearCookies(): number {
  if (typeof document === 'undefined' || !document.cookie) {
    return 0;
  }
  const names = document.cookie
    .split(';')
    .map((part) => part.split('=')[0]?.trim())
    .filter((name): name is string => Boolean(name));
  for (const name of names) {
    document.cookie = `${name}=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/`;
  }
  return names.length;
}

function deleteDatabase(idb: IDBFactory, name: string): Promise<'deleted' | 'blocked'> {
  return new Promise((resolve, reject) => {
    const request = idb.deleteDatabase(name);
    request.onsuccess = () => resolve('deleted');
    request.onblocked = () => resolve('blocked');
    request.onerror = () => reject(new Error(`could not delete IndexedDB \`${name}\`: ${request.error?.message ?? 'unknown error'}`));
  });
}
