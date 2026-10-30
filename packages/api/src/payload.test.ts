import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it, vi } from 'vitest';
import {
  DEFAULT_REDACTION_KEYS,
  MAX_REDACTION_NODES,
  payloadPolicy,
  recordedConsole,
  recordedError,
  recordedErrorMessage,
  recordedPayload,
  recordPayload,
  redactKeys,
  REDACTED,
} from './payload.js';

vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => ({ label: 'main' }) }));

describe('payload policy', () => {
  it('keeps the default keys identical to the Rust plugin', () => {
    const file = fileURLToPath(new URL('../../../crates/auditaur-core/src/redaction.rs', import.meta.url));
    const block = /DEFAULT_REDACTION_KEYS: &\[&str\] = &\[([\s\S]*?)\];/.exec(readFileSync(file, 'utf8'));
    const rust = [...(block?.[1] ?? '').matchAll(/"([^"]+)"/g)].map((match) => match[1]);
    expect(rust.length).toBeGreaterThan(0);
    expect([...DEFAULT_REDACTION_KEYS]).toEqual(rust);
  });

  it('redacts default keys in summaries at any depth, case-insensitively', () => {
    const recorded = recordPayload(payloadPolicy(), 'invokeArgs', 'login', {
      user: 'ada',
      Password: 'hunter2',
      nested: [{ access_token: 'abc', keep: 1 }],
    });
    expect(recorded.summary).toBe(
      '{"user":"ada","Password":"[REDACTED]","nested":[{"access_token":"[REDACTED]","keep":1}]}',
    );
    expect(recorded.json).toBeUndefined();
  });

  it('redacts configured keys in summaries and full payloads', () => {
    const policy = payloadPolicy({ captureFullPayloads: true, redactKeys: ['Email'] });
    const recorded = recordPayload(policy, 'invokeResult', 'list_variations', [{ author: 'a', email: 'a@b.c' }]);
    expect(recorded.summary).toBe('[{"author":"a","email":"[REDACTED]"}]');
    expect(recorded.json).toEqual([{ author: 'a', email: REDACTED }]);
  });

  it('records nothing when summaries and full payloads are off', () => {
    const recorded = recordPayload(payloadPolicy({ payloadSummaries: false }), 'eventPayload', 'chat', { text: 'hi' });
    expect(recorded).toEqual({});
  });

  it('applies the redactPayload hook with context before key redaction', () => {
    const redactPayload = vi.fn((payload: unknown, context: { kind: string; name: string }) =>
      context.name === 'save_chat_session' ? { transcript: '[omitted]', token: 't' } : payload,
    );
    const recorded = recordPayload(payloadPolicy({ redactPayload }), 'invokeArgs', 'save_chat_session', {
      transcript: 'secret chat',
    });
    expect(redactPayload).toHaveBeenCalledWith({ transcript: 'secret chat' }, { kind: 'invokeArgs', name: 'save_chat_session' });
    expect(recorded.summary).toBe('{"transcript":"[omitted]","token":"[REDACTED]"}');
  });

  it('fails closed when the hook throws', () => {
    const policy = payloadPolicy({
      captureFullPayloads: true,
      redactPayload: () => {
        throw new Error('boom');
      },
    });
    expect(recordPayload(policy, 'invokeArgs', 'x', { text: 'private' })).toEqual({ summary: REDACTED, json: REDACTED });
  });

  it('walks values the way JSON.stringify does', () => {
    class Credentials {
      user = 'ada';
      token = 't';
    }
    const cyclic: Record<string, unknown> = { secret: 's' };
    cyclic.self = cyclic;
    const keys = payloadPolicy().keys;
    expect(redactKeys(new Credentials(), keys)).toEqual({ user: 'ada', token: REDACTED });
    expect(redactKeys({ at: new Date(0) }, keys)).toEqual({ at: '1970-01-01T00:00:00.000Z' });
    expect(redactKeys(cyclic, keys)).toEqual({ secret: REDACTED, self: '[Circular]' });
    expect(redactKeys('token', keys)).toBe('token');
  });

  it('fails closed when walking the payload throws, without throwing into the app', () => {
    const payload = {
      get secretive() {
        throw new Error('getter');
      },
    };
    expect(recordPayload(payloadPolicy(), 'invokeResult', 'x', payload)).toEqual({ summary: REDACTED, json: undefined });
    const throwingToJson = { toJSON: () => { throw new Error('toJSON'); } };
    expect(recordPayload(payloadPolicy(), 'eventPayload', 'x', throwingToJson)).toEqual({ summary: REDACTED, json: undefined });
  });

  it('bounds traversal of self-returning toJSON and huge payloads', () => {
    const keys = payloadPolicy().keys;
    const selfish = { toJSON(): unknown { return this; } };
    expect(redactKeys(selfish, keys)).toBe('[Circular]');
    const huge = Array.from({ length: MAX_REDACTION_NODES + 5 }, (_, i) => ({ i }));
    const walked = redactKeys(huge, keys) as unknown[];
    expect(walked.at(-1)).toBe('[Truncated]');
    expect(walked[0]).toEqual({ i: 0 });
  });
});

describe('error message redaction', () => {
  it('records the message as-is by default', () => {
    expect(recordedErrorMessage({}, 'save', new Error('disk full'))).toBe('disk full');
    expect(recordedErrorMessage({}, 'save', 'plain string')).toBe('plain string');
  });

  it('key-redacts object errors before the hook sees them', () => {
    const hook = vi.fn((message: string) => message);
    const message = recordedErrorMessage({ redactErrorMessage: hook, redactKeys: ['email'] }, 'list', {
      kind: 'conflict',
      email: 'ada@example.com',
    });
    expect(message).toBe('{"kind":"conflict","email":"[REDACTED]"}');
    expect(hook).toHaveBeenCalledWith(message, { name: 'list' });
  });

  it('applies the hook', () => {
    const config = { redactErrorMessage: (m: string) => m.replace(/\S+@\S+/g, '<email>') };
    expect(recordedErrorMessage(config, 'save', new Error('no access for ada@example.com'))).toBe('no access for <email>');
  });

  it('fails closed when the hook throws or returns a non-string', () => {
    const throws = { redactErrorMessage: () => { throw new Error('bug'); } };
    expect(recordedErrorMessage(throws, 'save', new Error('secret path'))).toBe(REDACTED);
    const wrong = { redactErrorMessage: (() => undefined) as unknown as (m: string) => string };
    expect(recordedErrorMessage(wrong, 'save', new Error('secret path'))).toBe(REDACTED);
  });

  it('recordedPayload matches what recording keeps', () => {
    const config = { redactKeys: ['author'], redactPayload: (p: unknown) => ({ ...(p as object), transcript: undefined }) };
    expect(recordedPayload(config, 'invokeResult', 'history', { author: { email: 'a@b' }, transcript: 'hi', n: 1 })).toEqual({
      summary: '{"author":"[REDACTED]","n":1}',
      json: undefined,
    });
  });
});

describe('console and uncaught error redaction', () => {
  const email = (m: string) => m.replace(/\S+@\S+/g, '<email>');

  it('key-redacts console arguments and keeps their JSON only with captureFullPayloads', () => {
    const args = ['saved', { user: 'ada', token: 't0k' }];
    const plain = recordedConsole({}, 'log', args);
    expect(plain.body).toBe(`saved {"user":"ada","token":"${REDACTED}"}`);
    expect(plain.bodyJson).toBeUndefined();
    expect(recordedConsole({ captureFullPayloads: true }, 'log', args).bodyJson).toEqual({ user: 'ada', token: REDACTED });
  });

  it('runs redactConsole over the joined body, failing closed', () => {
    expect(recordedConsole({ redactConsole: (body, { level }) => `${level}:${email(body)}` }, 'warn', ['to ada@example.com']).body)
      .toBe('warn:to <email>');
    expect(recordedConsole({ redactConsole: () => { throw new Error('bug'); } }, 'log', ['secret']).body).toBe(REDACTED);
    expect(recordedConsole({ redactConsole: (() => 1) as unknown as (b: string) => string }, 'log', ['secret']).body).toBe(REDACTED);
  });

  it('rewrites an error message in the message, the stack, and a console body', () => {
    const error = new Error('no access for ada@example.com');
    const name = vi.fn((m: string, _context: { name: string }) => email(m));
    const recorded = recordedError({ redactErrorMessage: name }, 'unhandledrejection', error);
    expect(name).toHaveBeenCalledWith('no access for ada@example.com', { name: 'unhandledrejection' });
    expect(recorded.message).toBe('no access for <email>');
    expect(recorded.stack).toContain('Error: no access for <email>');
    expect(recorded.stack).not.toContain('ada@example.com');

    const logged = recordedConsole({ redactErrorMessage: email }, 'error', ['failed:', error]);
    expect(logged.body).not.toContain('ada@example.com');
    expect(logged.error?.message).toBe('no access for <email>');
  });

  it('drops a stack that does not visibly carry the rewritten message', () => {
    const error = new Error('ada@example.com');
    error.stack = 'at save (app.js:1:1)\nada%40example.com';
    const recorded = recordedError({ redactErrorMessage: email }, 'window.error', error);
    expect(recorded).toEqual({ message: '<email>', stack: undefined, errorType: 'Error' });
  });

  it('leaves errors alone without a hook, and key-redacts object rejections', () => {
    const error = new Error('plain');
    expect(recordedError({}, 'window.error', error).stack).toBe(error.stack);
    expect(recordedError({}, 'unhandledrejection', { reason: 'x', token: 't0k' }).message).toBe(`{"reason":"x","token":"${REDACTED}"}`);
  });
});