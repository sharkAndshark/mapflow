import { describe, expect, it } from 'vitest';

import { formatInspectorValue } from '../../src/featureInspectorFormat.js';

describe('feature inspector formatting', () => {
  it('formats NULL as placeholder', () => {
    expect(formatInspectorValue(null)).toEqual({ text: '--', title: 'NULL', tone: 'null' });
  });

  it('formats empty string distinctly', () => {
    expect(formatInspectorValue('')).toEqual({ text: '""', title: 'Empty string', tone: 'empty' });
  });

  it('formats non-empty values as strings', () => {
    expect(formatInspectorValue('abc')).toEqual({ text: 'abc', title: undefined, tone: 'value' });
    expect(formatInspectorValue(0)).toEqual({ text: '0', title: undefined, tone: 'value' });
    expect(formatInspectorValue(false)).toEqual({ text: 'false', title: undefined, tone: 'value' });
  });
});
