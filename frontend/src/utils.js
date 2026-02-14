export function formatSize(bytes) {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / Math.pow(1024, index);
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[index]}`;
}

export function parseType(fileName) {
  const lower = fileName.toLowerCase();
  if (lower.endsWith('.zip')) return 'shapefile';
  if (lower.endsWith('.geojson') || lower.endsWith('.json')) return 'geojson';
  if (lower.endsWith('.geojsonl') || lower.endsWith('.geojsons')) return 'geojsonl';
  if (lower.endsWith('.kml')) return 'kml';
  if (lower.endsWith('.gpx')) return 'gpx';
  if (lower.endsWith('.topojson')) return 'topojson';
  if (lower.endsWith('.mbtiles')) return 'mbtiles';
  return 'unknown';
}

export function validateSlug(slug) {
  if (!slug) return { valid: true, error: '' };
  if (slug.length > 100) return { valid: false, error: 'URL 标识不能超过 100 个字符' };
  if (!/^[a-zA-Z0-9_-]+$/.test(slug)) {
    return { valid: false, error: '仅支持字母、数字、连字符和下划线' };
  }
  return { valid: true, error: '' };
}
