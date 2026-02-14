import { describe, expect, it } from 'vitest';

import { formatSize, parseType, validateSlug } from '../../src/utils.js';

describe('formatSize', () => {
  it('formats 0 bytes', () => {
    expect(formatSize(0)).toBe('0 B');
  });

  it('formats bytes', () => {
    expect(formatSize(100)).toBe('100 B');
    expect(formatSize(1023)).toBe('1023 B');
  });

  it('formats kilobytes', () => {
    expect(formatSize(1024)).toBe('1.0 KB');
    expect(formatSize(1536)).toBe('1.5 KB');
    expect(formatSize(10240)).toBe('10 KB');
  });

  it('formats megabytes', () => {
    expect(formatSize(1048576)).toBe('1.0 MB');
    expect(formatSize(10485760)).toBe('10 MB');
  });

  it('formats gigabytes', () => {
    expect(formatSize(1073741824)).toBe('1.0 GB');
    expect(formatSize(10737418240)).toBe('10 GB');
  });
});

describe('parseType', () => {
  it('recognizes shapefile', () => {
    expect(parseType('data.zip')).toBe('shapefile');
    expect(parseType('DATA.ZIP')).toBe('shapefile');
  });

  it('recognizes geojson', () => {
    expect(parseType('data.geojson')).toBe('geojson');
    expect(parseType('data.json')).toBe('geojson');
    expect(parseType('DATA.GEOJSON')).toBe('geojson');
  });

  it('recognizes geojsonl', () => {
    expect(parseType('data.geojsonl')).toBe('geojsonl');
    expect(parseType('data.geojsons')).toBe('geojsonl');
  });

  it('recognizes kml', () => {
    expect(parseType('data.kml')).toBe('kml');
  });

  it('recognizes gpx', () => {
    expect(parseType('data.gpx')).toBe('gpx');
  });

  it('recognizes topojson', () => {
    expect(parseType('data.topojson')).toBe('topojson');
  });

  it('recognizes mbtiles', () => {
    expect(parseType('data.mbtiles')).toBe('mbtiles');
  });

  it('returns unknown for unrecognized extensions', () => {
    expect(parseType('data.txt')).toBe('unknown');
    expect(parseType('data')).toBe('unknown');
  });
});

describe('validateSlug', () => {
  it('accepts empty slug', () => {
    expect(validateSlug('')).toEqual({ valid: true, error: '' });
  });

  it('accepts valid slugs', () => {
    expect(validateSlug('my-map')).toEqual({ valid: true, error: '' });
    expect(validateSlug('my_map')).toEqual({ valid: true, error: '' });
    expect(validateSlug('map123')).toEqual({ valid: true, error: '' });
    expect(validateSlug('ABC')).toEqual({ valid: true, error: '' });
    expect(validateSlug('a')).toEqual({ valid: true, error: '' });
  });

  it('rejects slug longer than 100 characters', () => {
    const longSlug = 'a'.repeat(101);
    expect(validateSlug(longSlug)).toEqual({
      valid: false,
      error: 'URL 标识不能超过 100 个字符',
    });
  });

  it('accepts slug exactly 100 characters', () => {
    const maxSlug = 'a'.repeat(100);
    expect(validateSlug(maxSlug)).toEqual({ valid: true, error: '' });
  });

  it('rejects slug with invalid characters', () => {
    expect(validateSlug('my map')).toEqual({
      valid: false,
      error: '仅支持字母、数字、连字符和下划线',
    });
    expect(validateSlug('my.map')).toEqual({
      valid: false,
      error: '仅支持字母、数字、连字符和下划线',
    });
    expect(validateSlug('my/map')).toEqual({
      valid: false,
      error: '仅支持字母、数字、连字符和下划线',
    });
    expect(validateSlug('my!map')).toEqual({
      valid: false,
      error: '仅支持字母、数字、连字符和下划线',
    });
  });
});
