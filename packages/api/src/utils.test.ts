import { describe, expect, it } from 'vitest';
import { consoleLogRecord } from './console';
import { errorRecordFields, summarizePayload } from './utils';

describe('payload summaries', () => {
  it('bounds string payloads', () => {
    expect(summarizePayload('abcdef', 3)).toBe('abc');
  });

  it('summarizes console args', () => {
    const record = consoleLogRecord('info', ['hello', { answer: 42 }], 100);
    expect(record.severityText).toBe('INFO');
    expect(record.body).toContain('hello');
    expect(record.body).toContain('"answer":42');
  });

  it('extracts error fields', () => {
    const fields = errorRecordFields(new TypeError('bad'));
    expect(fields.message).toBe('bad');
    expect(fields.errorType).toBe('TypeError');
  });

  it('summarizes errors with message and stack', () => {
    const error = new Error('boom');
    expect(summarizePayload(error, 1_000)).toContain('Error: boom');
  });
});
