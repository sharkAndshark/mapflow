export function getFeatureFid(feature) {
  return feature?.getId?.() ?? feature?.get?.('fid') ?? feature?.getProperties?.()?.fid;
}

export function hasValidFid(fid) {
  return fid !== undefined && fid !== null && fid !== '';
}

export function extractInspectableFeatureProperties(feature) {
  const props = feature?.getProperties?.() ?? {};
  return Object.entries(props)
    .filter(([key]) => !['geometry', 'fid', 'layerName'].includes(key))
    .map(([key, value]) => ({ key, value }));
}
