import { Fill, Stroke, Style, Circle as CircleStyle } from 'ol/style';

const LAYER_TYPE_MAP = {
  fill: 'polygon',
  line: 'line',
  circle: 'point',
  symbol: 'point',
};

const DEFAULT_PAINT = {
  fill: { 'fill-color': '#0080ff', 'fill-opacity': 0.6 },
  line: { 'line-color': '#0080ff', 'line-width': 2 },
  circle: {
    'circle-radius': 6,
    'circle-color': '#ff0040',
    'circle-stroke-color': '#ffffff',
    'circle-stroke-width': 1,
  },
};

function isExpression(val) {
  return Array.isArray(val) && val.length > 0 && typeof val[0] === 'string';
}

function evalExpr(expr, feature) {
  if (!Array.isArray(expr)) return expr;
  const op = expr[0];

  if (op === 'get') return feature.get(expr[1]);
  if (op === 'literal') return expr[1];

  if (op === '==') return evalExpr(expr[1], feature) == evalExpr(expr[2], feature);
  if (op === '!=') return evalExpr(expr[1], feature) != evalExpr(expr[2], feature);
  if (op === '<') return evalExpr(expr[1], feature) < evalExpr(expr[2], feature);
  if (op === '<=') return evalExpr(expr[1], feature) <= evalExpr(expr[2], feature);
  if (op === '>') return evalExpr(expr[1], feature) > evalExpr(expr[2], feature);
  if (op === '>=') return evalExpr(expr[1], feature) >= evalExpr(expr[2], feature);

  if (op === 'case') {
    for (let i = 1; i < expr.length - 1; i += 2) {
      if (evalExpr(expr[i], feature)) return evalExpr(expr[i + 1], feature);
    }
    return evalExpr(expr[expr.length - 1], feature);
  }

  if (op === 'match') {
    const input = evalExpr(expr[1], feature);
    for (let i = 2; i < expr.length - 1; i += 2) {
      const labels = Array.isArray(expr[i]) ? expr[i] : [expr[i]];
      if (labels.includes(input)) return evalExpr(expr[i + 1], feature);
    }
    return evalExpr(expr[expr.length - 1], feature);
  }

  if (op === 'interpolate') {
    const type = expr[1];
    const input = evalExpr(expr[2], feature);
    const stops = expr.slice(3);
    for (let i = 0; i < stops.length - 3; i += 2) {
      const lo = stops[i];
      const hi = stops[i + 2];
      const loVal = stops[i + 1];
      const hiVal = stops[i + 3];
      if (input >= lo && input <= hi) {
        const t = (input - lo) / (hi - lo);
        if (type[0] === 'linear') return loVal + t * (hiVal - loVal);
      }
    }
    return stops.length >= 2 ? stops[stops.length - 1] : input;
  }

  if (op === 'coalesce') {
    for (let i = 1; i < expr.length; i++) {
      const v = evalExpr(expr[i], feature);
      if (v != null) return v;
    }
    return null;
  }

  return expr;
}

function hasExpression(paint) {
  if (!paint || typeof paint !== 'object') return false;
  return Object.values(paint).some(isExpression);
}

function applyOpacity(color, opacity) {
  const rgba = parseColor(color);
  if (!rgba) return color;
  return `rgba(${rgba.r}, ${rgba.g}, ${rgba.b}, ${(rgba.a * opacity).toFixed(2)})`;
}

function parseColor(color) {
  if (!color || typeof color !== 'string') return null;

  const hexMatch = color.match(/^#([0-9a-f]{6})$/i);
  if (hexMatch) {
    const hex = hexMatch[1];
    return {
      r: parseInt(hex.substring(0, 2), 16),
      g: parseInt(hex.substring(2, 4), 16),
      b: parseInt(hex.substring(4, 6), 16),
      a: 1,
    };
  }

  const rgbaMatch = color.match(
    /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([0-9.]+)\s*)?\)$/,
  );
  if (rgbaMatch) {
    return {
      r: parseInt(rgbaMatch[1], 10),
      g: parseInt(rgbaMatch[2], 10),
      b: parseInt(rgbaMatch[3], 10),
      a: rgbaMatch[4] !== undefined ? parseFloat(rgbaMatch[4]) : 1,
    };
  }

  return null;
}

function resolveColor(raw, opacityProp) {
  if (typeof raw !== 'string') return raw;
  if (typeof opacityProp === 'number') {
    return applyOpacity(raw, opacityProp);
  }
  return raw;
}

function staticFillStyle(p) {
  const fillColor = resolveColor(
    p['fill-color'] || DEFAULT_PAINT.fill['fill-color'],
    p['fill-opacity'] ?? DEFAULT_PAINT.fill['fill-opacity'],
  );
  const outlineColor = p['fill-outline-color'] || null;
  return new Style({
    fill: new Fill({ color: fillColor }),
    stroke: outlineColor ? new Stroke({ color: outlineColor, width: 1 }) : undefined,
  });
}

function staticLineStyle(p) {
  const lineColor = resolveColor(
    p['line-color'] || DEFAULT_PAINT.line['line-color'],
    p['line-opacity'],
  );
  return new Style({
    stroke: new Stroke({
      color: lineColor,
      width: p['line-width'] ?? DEFAULT_PAINT.line['line-width'],
      lineDash: p['line-dasharray'] || undefined,
    }),
  });
}

function staticCircleStyle(p) {
  const dp = DEFAULT_PAINT.circle;
  const circleColor = resolveColor(
    p['circle-color'] || dp['circle-color'],
    p['circle-opacity'],
  );
  return new Style({
    image: new CircleStyle({
      radius: p['circle-radius'] ?? dp['circle-radius'],
      fill: new Fill({ color: circleColor }),
      stroke: new Stroke({
        color: p['circle-stroke-color'] || dp['circle-stroke-color'],
        width: p['circle-stroke-width'] ?? dp['circle-stroke-width'],
      }),
    }),
  });
}

function paintToOlStyle(layerType, paint) {
  const p = paint || {};

  if (!hasExpression(p)) {
    if (layerType === 'fill') return staticFillStyle(p);
    if (layerType === 'line') return staticLineStyle(p);
    if (layerType === 'circle') return staticCircleStyle(p);
    return staticFillStyle(p);
  }

  return (feature) => {
    const resolved = {};
    for (const [key, val] of Object.entries(p)) {
      resolved[key] = isExpression(val) ? evalExpr(val, feature) : val;
    }

    if (layerType === 'fill') {
      const fillColor = resolveColor(
        resolved['fill-color'] || DEFAULT_PAINT.fill['fill-color'],
        resolved['fill-opacity'] ?? DEFAULT_PAINT.fill['fill-opacity'],
      );
      return new Style({
        fill: new Fill({ color: fillColor }),
        stroke: resolved['fill-outline-color']
          ? new Stroke({ color: resolved['fill-outline-color'], width: 1 })
          : undefined,
      });
    }

    if (layerType === 'line') {
      const lineColor = resolveColor(
        resolved['line-color'] || DEFAULT_PAINT.line['line-color'],
        resolved['line-opacity'],
      );
      return new Style({
        stroke: new Stroke({
          color: lineColor,
          width: resolved['line-width'] ?? DEFAULT_PAINT.line['line-width'],
          lineDash: resolved['line-dasharray'] || undefined,
        }),
      });
    }

    if (layerType === 'circle') {
      const dp = DEFAULT_PAINT.circle;
      const circleColor = resolveColor(
        resolved['circle-color'] || dp['circle-color'],
        resolved['circle-opacity'],
      );
      return new Style({
        image: new CircleStyle({
          radius: resolved['circle-radius'] ?? dp['circle-radius'],
          fill: new Fill({ color: circleColor }),
          stroke: new Stroke({
            color: resolved['circle-stroke-color'] || dp['circle-stroke-color'],
            width: resolved['circle-stroke-width'] ?? dp['circle-stroke-width'],
          }),
        }),
      });
    }

    return staticFillStyle(resolved);
  };
}

function createTileUrl(sourceId) {
  return `${window.location.origin}/api/files/${sourceId}/tiles/{z}/{x}/{y}`;
}

function parseDataBounds(boundsStr) {
  if (!boundsStr) return null;
  try {
    const parsed = JSON.parse(boundsStr);
    if (Array.isArray(parsed) && parsed.length === 4) return parsed;
  } catch {}
  return null;
}

export { DEFAULT_PAINT, LAYER_TYPE_MAP, paintToOlStyle };

export function styleJsonToOlLayers(styleJson, sourceMeta) {
  if (!styleJson || !styleJson.layers) return [];

  return styleJson.layers.map((layer) => {
    const sourceId = layer.source;
    const geomType = LAYER_TYPE_MAP[layer.type] || 'polygon';
    const olStyle = paintToOlStyle(layer.type, layer.paint);

    const meta = sourceMeta?.[sourceId];
    const dataBounds = meta?.dataBounds ? parseDataBounds(meta.dataBounds) : null;
    const isCustomCRS = meta?.crsType === 'custom' && dataBounds;

    return {
      id: layer.id,
      sourceId,
      geomType,
      olStyle,
      visible: layer.layout?.visibility !== 'none',
      isCustomCRS,
      customCRS: isCustomCRS ? meta.crs : null,
      dataBounds: isCustomCRS ? dataBounds : null,
    };
  });
}

export function buildEmptyStyleJson() {
  return {
    version: 8,
    sources: {},
    layers: [],
  };
}

export function addSourceToStyle(styleJson, sourceId, meta) {
  const result = { ...styleJson };
  result.sources = { ...result.sources };

  const source = {
    type: 'vector',
    tiles: [createTileUrl(sourceId)],
    minzoom: 0,
    maxzoom: 20,
  };

  if (meta?.crsType === 'custom') {
    source['_mapflow:crsType'] = 'custom';
    if (meta?.dataBounds) {
      source['_mapflow:dataBounds'] = parseDataBounds(meta.dataBounds) || meta.dataBounds;
    }
  }

  result.sources[sourceId] = source;
  return result;
}

export function addLayerToStyle(styleJson, sourceId, layerType, paint) {
  const result = { ...styleJson };
  result.layers = [...result.layers];

  const layerId = `${sourceId}-${layerType}-${Date.now()}`;
  result.layers.push({
    id: layerId,
    type: layerType,
    source: sourceId,
    paint: paint || {},
  });

  return result;
}

export function removeLayerFromStyle(styleJson, layerId) {
  const newLayers = styleJson.layers.filter((l) => l.id !== layerId);
  const removedLayer = styleJson.layers.find((l) => l.id === layerId);
  const newSources = { ...styleJson.sources };
  if (removedLayer) {
    const stillUsed = newLayers.some((l) => l.source === removedLayer.source);
    if (!stillUsed) delete newSources[removedLayer.source];
  }
  return { ...styleJson, sources: newSources, layers: newLayers };
}

export function updateLayerPaint(styleJson, layerId, paintUpdates) {
  return {
    ...styleJson,
    layers: styleJson.layers.map((l) =>
      l.id === layerId
        ? { ...l, paint: { ...(l.paint || {}), ...paintUpdates } }
        : l,
    ),
  };
}

export function moveLayer(styleJson, layerId, direction) {
  const layers = [...styleJson.layers];
  const idx = layers.findIndex((l) => l.id === layerId);
  if (idx < 0) return styleJson;

  const newIdx = direction === 'up' ? idx + 1 : idx - 1;
  if (newIdx < 0 || newIdx >= layers.length) return styleJson;

  [layers[idx], layers[newIdx]] = [layers[newIdx], layers[idx]];
  return { ...styleJson, layers };
}

export function setLayerVisibility(styleJson, layerId, visible) {
  return {
    ...styleJson,
    layers: styleJson.layers.map((l) =>
      l.id === layerId
        ? { ...l, layout: { ...(l.layout || {}), visibility: visible ? 'visible' : 'none' } }
        : l,
    ),
  };
}
