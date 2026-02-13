import { describe, expect, it } from 'vitest';

import {
  extractInspectableFeatureProperties,
  getFeatureFid,
  hasValidFid,
} from '../../src/previewFeatureProperties.js';

describe('preview feature properties helpers', () => {
  it('keeps mvt feature properties even when fid is missing', () => {
    const feature = {
      getId: () => undefined,
      get: () => undefined,
      getProperties: () => ({
        name: 'main-road',
        speed: 50,
        fid: null,
        layerName: 'roads',
        geometry: { type: 'LineString' },
      }),
    };

    const fid = getFeatureFid(feature);
    expect(hasValidFid(fid)).toBe(false);
    expect(extractInspectableFeatureProperties(feature)).toEqual([
      { key: 'name', value: 'main-road' },
      { key: 'speed', value: 50 },
    ]);
  });

  it('treats numeric zero fid as valid', () => {
    expect(hasValidFid(0)).toBe(true);
  });
});
