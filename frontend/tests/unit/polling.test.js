import { describe, expect, it } from 'vitest';

import { hasActiveJobs, mergeServerFilesWithOptimistic } from '../../src/polling.js';

describe('polling helpers', () => {
  it('hasActiveJobs: true when uploaded/processing exists', () => {
    expect(hasActiveJobs([{ status: 'ready' }])).toBe(false);
    expect(hasActiveJobs([{ status: 'uploaded' }])).toBe(true);
    expect(hasActiveJobs([{ status: 'processing' }])).toBe(true);
    expect(hasActiveJobs([{ status: 'failed' }])).toBe(false);
  });

  it('mergeServerFilesWithOptimistic: keeps uploading optimistic not present on server', () => {
    const prev = [
      { id: 'temp-1', status: 'uploading', name: 'a' },
      { id: 's-1', status: 'uploaded', name: 'b' },
    ];
    const server = [{ id: 's-1', status: 'ready', name: 'b' }];

    expect(mergeServerFilesWithOptimistic(prev, server)).toEqual([
      { id: 'temp-1', status: 'uploading', name: 'a' },
      { id: 's-1', status: 'ready', name: 'b' },
    ]);
  });

  it('mergeServerFilesWithOptimistic: drops uploading optimistic once server has same id', () => {
    const prev = [{ id: 'same', status: 'uploading', name: 'x' }];
    const server = [{ id: 'same', status: 'uploaded', name: 'x' }];

    expect(mergeServerFilesWithOptimistic(prev, server)).toEqual([
      { id: 'same', status: 'uploaded', name: 'x' },
    ]);
  });
});
