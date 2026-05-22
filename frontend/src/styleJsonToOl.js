import { Fill, Stroke, Style, Circle as CircleStyle, Text } from 'ol/style';

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

const LINE_DASH_MAP = {
  solid: null,
  dashed: [10, 5],
  dotted: [2, 5],
  dashdot: [10, 5, 2, 5],
};

function isExpression(val) {
  return Array.isArray(val) && val.length > 0 && typeof val[0] === 'string';
}

function evalExpr(expr, feature, resolution) {
  if (!Array.isArray(expr)) return expr;
  const op = expr[0];

  if (op === 'get') return feature.get(expr[1]);
  if (op === 'literal') return expr[1];

  if (op === 'zoom') {
    if (resolution == null) return 0;
    return resolutionToZoom(resolution);
  }

  if (op === '==')
    return evalExpr(expr[1], feature, resolution) === evalExpr(expr[2], feature, resolution);
  if (op === '!=')
    return evalExpr(expr[1], feature, resolution) !== evalExpr(expr[2], feature, resolution);
  if (op === '<')
    return evalExpr(expr[1], feature, resolution) < evalExpr(expr[2], feature, resolution);
  if (op === '<=')
    return evalExpr(expr[1], feature, resolution) <= evalExpr(expr[2], feature, resolution);
  if (op === '>')
    return evalExpr(expr[1], feature, resolution) > evalExpr(expr[2], feature, resolution);
  if (op === '>=')
    return evalExpr(expr[1], feature, resolution) >= evalExpr(expr[2], feature, resolution);

  if (op === 'all') {
    for (let i = 1; i < expr.length; i++) {
      if (!evalExpr(expr[i], feature, resolution)) return false;
    }
    return true;
  }
  if (op === 'any') {
    for (let i = 1; i < expr.length; i++) {
      if (evalExpr(expr[i], feature, resolution)) return true;
    }
    return false;
  }
  if (op === '!') return !evalExpr(expr[1], feature, resolution);

  if (op === 'in') {
    const val = evalExpr(expr[1], feature, resolution);
    for (let i = 2; i < expr.length; i++) {
      if (val === expr[i]) return true;
    }
    return false;
  }

  if (op === 'case') {
    for (let i = 1; i < expr.length - 1; i += 2) {
      if (evalExpr(expr[i], feature, resolution)) return evalExpr(expr[i + 1], feature, resolution);
    }
    return evalExpr(expr[expr.length - 1], feature, resolution);
  }

  if (op === 'match') {
    const input = evalExpr(expr[1], feature, resolution);
    for (let i = 2; i < expr.length - 1; i += 2) {
      const labels = Array.isArray(expr[i]) ? expr[i] : [expr[i]];
      if (labels.includes(input)) return evalExpr(expr[i + 1], feature, resolution);
    }
    return evalExpr(expr[expr.length - 1], feature, resolution);
  }

  if (op === 'interpolate') {
    const type = expr[1];
    const input = evalExpr(expr[2], feature, resolution);
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
    return stops.length >= 2 ? (input <= stops[0] ? stops[1] : stops[stops.length - 1]) : input;
  }

  if (op === 'coalesce') {
    for (let i = 1; i < expr.length; i++) {
      const v = evalExpr(expr[i], feature, resolution);
      if (v != null) return v;
    }
    return null;
  }

  if (op === 'concat') {
    let result = '';
    for (let i = 1; i < expr.length; i++) {
      const v = evalExpr(expr[i], feature, resolution);
      result += v != null ? String(v) : '';
    }
    return result;
  }

  if (op === 'to-string') {
    const v = evalExpr(expr[1], feature, resolution);
    return v != null ? String(v) : '';
  }

  return expr;
}

function resolutionToZoom(resolution) {
  if (resolution == null || resolution <= 0) return 0;
  const z = Math.log2(156543.03392804097 / resolution);
  return Math.max(0, Math.round(z));
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

function resolveLineDash(lineStyle) {
  if (!lineStyle || lineStyle === 'solid') return undefined;
  return LINE_DASH_MAP[lineStyle] || undefined;
}

function buildFillStyle(p) {
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

function buildLineStyle(p) {
  const lineColor = resolveColor(
    p['line-color'] || DEFAULT_PAINT.line['line-color'],
    p['line-opacity'],
  );
  const lineCap = p['line-cap'] || 'butt';
  const lineJoin = p['line-join'] || 'mitre';
  return new Style({
    stroke: new Stroke({
      color: lineColor,
      width: p['line-width'] ?? DEFAULT_PAINT.line['line-width'],
      lineDash: resolveLineDash(p['_lineStyle']) || p['line-dasharray'] || undefined,
      lineCap,
      lineJoin,
    }),
  });
}

function buildCircleStyle(p) {
  const dp = DEFAULT_PAINT.circle;
  const circleColor = resolveColor(p['circle-color'] || dp['circle-color'], p['circle-opacity']);
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

function resolvePaintProperties(paint, feature, resolution) {
  const resolved = {};
  for (const [key, val] of Object.entries(paint)) {
    if (key.startsWith('_')) continue;
    resolved[key] = isExpression(val) ? evalExpr(val, feature, resolution) : val;
  }
  if (paint['_lineStyle']) {
    resolved['_lineStyle'] = paint['_lineStyle'];
  }
  return resolved;
}

function paintToOlStyle(layerType, paint) {
  const p = paint || {};

  if (!hasExpression(p)) {
    if (layerType === 'fill') return buildFillStyle(p);
    if (layerType === 'line') return buildLineStyle(p);
    if (layerType === 'circle') return buildCircleStyle(p);
    return buildFillStyle(p);
  }

  return (feature, resolution) => {
    const resolved = resolvePaintProperties(p, feature, resolution);

    if (layerType === 'fill') {
      return buildFillStyle(resolved);
    }
    if (layerType === 'line') {
      return buildLineStyle(resolved);
    }
    if (layerType === 'circle') {
      return buildCircleStyle(resolved);
    }
    return buildFillStyle(resolved);
  };
}

function rendererToOlStyle(renderer, layerType) {
  if (!renderer || renderer.type === 'none') return null;

  const paint = rendererToPaint(renderer, layerType);
  return paintToOlStyle(layerType, paint);
}

function labelToOlStyle(label) {
  if (!label || !label.enabled || !label.field) return null;

  const font = label.font || 'sans-serif';
  const size = label.size || 12;
  const color = label.color || '#333333';
  const haloColor = label.haloColor || '#ffffff';
  const haloWidth = label.haloWidth ?? 1;
  const offsetX = label.offsetX ?? 0;
  const offsetY = label.offsetY ?? 0;
  const maxAngle = label.maxAngle ?? 0;

  return (feature, resolution) => {
    const textVal = feature.get(label.field);
    if (textVal == null || textVal === '') return null;

    let fontSize = size;
    if (label.sizeByZoom && resolution != null) {
      const zoom = resolutionToZoom(resolution);
      for (let i = label.sizeByZoom.length - 1; i >= 0; i--) {
        if (zoom >= label.sizeByZoom[i][0]) {
          fontSize = label.sizeByZoom[i][1];
          break;
        }
      }
    }

    const text = String(textVal);
    return new Style({
      text: new Text({
        text,
        font: `${fontSize}px ${font}`,
        fill: new Fill({ color }),
        stroke: new Stroke({ color: haloColor, width: haloWidth }),
        offsetX,
        offsetY,
        placement: label.placement === 'line' ? 'line' : 'point',
        maxAngle,
        overflow: true,
      }),
    });
  };
}

function rendererToPaint(renderer, layerType) {
  if (!renderer || renderer.type === 'none') return null;

  if (renderer.type === 'single') {
    return buildSinglePaint(layerType, renderer);
  }
  if (renderer.type === 'categorized') {
    return buildCategorizedPaint(layerType, renderer);
  }
  if (renderer.type === 'graduated') {
    return buildGraduatedPaint(layerType, renderer);
  }
  if (renderer.type === 'proportional') {
    return buildProportionalPaint(layerType, renderer);
  }
  if (renderer.type === 'rules') {
    return buildRulesPaint(layerType, renderer);
  }
  return buildSinglePaint(layerType, renderer);
}

function buildSinglePaint(layerType, r) {
  const color = r.color || (layerType === 'circle' ? '#ff0040' : '#0080ff');
  const opacity = r.opacity ?? 1;
  const paint = {};

  if (layerType === 'fill') {
    paint['fill-color'] = color;
    paint['fill-opacity'] = opacity;
  } else if (layerType === 'line') {
    paint['line-color'] = color;
    paint['line-width'] = r.width ?? 2;
    paint['line-opacity'] = opacity;
    if (r.lineStyle && r.lineStyle !== 'solid') {
      paint['_lineStyle'] = r.lineStyle;
    }
    if (r.lineCap) paint['line-cap'] = r.lineCap;
    if (r.lineJoin) paint['line-join'] = r.lineJoin;
  } else if (layerType === 'circle') {
    paint['circle-color'] = color;
    paint['circle-radius'] = r.radius ?? 6;
    paint['circle-opacity'] = opacity;
    paint['circle-stroke-color'] = r.strokeColor || '#ffffff';
    paint['circle-stroke-width'] = r.strokeWidth ?? 1;
  }
  return paint;
}

function buildCategorizedPaint(layerType, r) {
  const fieldName = r.field;
  const classes = r.classes || [];
  if (!fieldName || classes.length === 0) return buildSinglePaint(layerType, r);

  const matchExpr = ['match', ['get', fieldName]];
  for (const c of classes) {
    matchExpr.push(c.value);
    matchExpr.push(c.color);
  }
  const fallback = classes[classes.length - 1]?.color || '#888888';
  matchExpr.push(fallback);

  const opacity = r.opacity ?? 1;
  const paint = {};
  if (layerType === 'fill') {
    paint['fill-color'] = matchExpr;
    paint['fill-opacity'] = opacity;
  } else if (layerType === 'line') {
    paint['line-color'] = matchExpr;
    paint['line-width'] = r.width ?? 2;
    paint['line-opacity'] = opacity;
    if (r.lineStyle && r.lineStyle !== 'solid') paint['_lineStyle'] = r.lineStyle;
    if (r.lineCap) paint['line-cap'] = r.lineCap;
    if (r.lineJoin) paint['line-join'] = r.lineJoin;
  } else if (layerType === 'circle') {
    paint['circle-color'] = matchExpr;
    paint['circle-radius'] = r.radius ?? 6;
    paint['circle-opacity'] = opacity;
  }
  return paint;
}

function buildGraduatedPaint(layerType, r) {
  const fieldName = r.field;
  const stops = r.stops || [];
  if (!fieldName || stops.length === 0) return buildSinglePaint(layerType, r);

  const cases = [];
  for (const stop of stops) {
    cases.push(['<=', ['get', fieldName], stop.value]);
    cases.push(stop.color);
  }
  const fallback = stops[stops.length - 1]?.color || '#888888';
  const opacity = r.opacity ?? 1;
  const paint = {};

  if (layerType === 'fill') {
    paint['fill-color'] = ['case', ...cases, fallback];
    paint['fill-opacity'] = opacity;
  } else if (layerType === 'line') {
    paint['line-color'] = ['case', ...cases, fallback];
    paint['line-width'] = r.width ?? 2;
    paint['line-opacity'] = opacity;
    if (r.lineStyle && r.lineStyle !== 'solid') paint['_lineStyle'] = r.lineStyle;
    if (r.lineCap) paint['line-cap'] = r.lineCap;
    if (r.lineJoin) paint['line-join'] = r.lineJoin;
  } else if (layerType === 'circle') {
    paint['circle-color'] = ['case', ...cases, fallback];
    paint['circle-radius'] = r.radius ?? 6;
    paint['circle-opacity'] = opacity;
  }
  return paint;
}

function buildProportionalPaint(layerType, r) {
  const fieldName = r.field;
  if (!fieldName || r.minVal == null || r.maxVal == null) return buildSinglePaint(layerType, r);

  const minR = r.minRadius ?? 3;
  const maxR = r.maxRadius ?? 25;
  const color = r.color || '#ff0040';
  const opacity = r.opacity ?? 0.8;

  const radiusExpr = [
    'interpolate',
    ['linear'],
    ['get', fieldName],
    r.minVal,
    minR,
    r.maxVal,
    maxR,
  ];
  const paint = {};

  if (layerType === 'circle') {
    paint['circle-color'] = color;
    paint['circle-radius'] = radiusExpr;
    paint['circle-opacity'] = opacity;
    paint['circle-stroke-color'] = '#ffffff';
    paint['circle-stroke-width'] = 1;
  } else if (layerType === 'fill') {
    paint['fill-color'] = color;
    paint['fill-opacity'] = opacity;
  } else if (layerType === 'line') {
    paint['line-color'] = color;
    paint['line-width'] = radiusExpr;
    paint['line-opacity'] = opacity;
  }
  return paint;
}

function filterToExpr(filterConfig) {
  if (!filterConfig?.conditions?.length) return null;
  const exprs = filterConfig.conditions
    .filter((c) => c.field && c.value !== '')
    .map((c) => {
      const fieldExpr = ['get', c.field];
      const val = isNaN(Number(c.value)) ? c.value : Number(c.value);
      if (c.operator === 'contains') {
        return ['in', val, ['to-string', fieldExpr]];
      }
      return [c.operator, fieldExpr, val];
    });
  if (exprs.length === 0) return null;
  if (exprs.length === 1) return exprs[0];
  return ['all', ...exprs];
}

function buildRulesPaint(layerType, r) {
  const rules = (r.rules || []).filter((rule) => rule.enabled !== false);
  if (rules.length === 0) return buildSinglePaint(layerType, r);

  const colorProp =
    layerType === 'fill' ? 'fill-color' : layerType === 'line' ? 'line-color' : 'circle-color';

  const cases = [];
  for (const rule of rules) {
    const filterExpr = filterToExpr(rule.filter);
    if (filterExpr) {
      cases.push(filterExpr);
      cases.push(rule.color || '#888888');
    }
  }
  const elseColor = r.elseColor || '#cccccc';

  const paint = {};
  const opacity = r.opacity ?? 0.7;

  if (layerType === 'fill') {
    paint['fill-color'] = cases.length > 0 ? ['case', ...cases, elseColor] : elseColor;
    paint['fill-opacity'] = opacity;
  } else if (layerType === 'line') {
    paint['line-color'] = cases.length > 0 ? ['case', ...cases, elseColor] : elseColor;
    paint['line-width'] = r.width ?? 2;
    paint['line-opacity'] = opacity;
    if (r.lineStyle && r.lineStyle !== 'solid') paint['_lineStyle'] = r.lineStyle;
    if (r.lineCap) paint['line-cap'] = r.lineCap;
    if (r.lineJoin) paint['line-join'] = r.lineJoin;
  } else if (layerType === 'circle') {
    paint['circle-color'] = cases.length > 0 ? ['case', ...cases, elseColor] : elseColor;
    paint['circle-radius'] = r.radius ?? 6;
    paint['circle-opacity'] = opacity;
  }
  return paint;
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

export {
  DEFAULT_PAINT,
  LAYER_TYPE_MAP,
  paintToOlStyle,
  rendererToOlStyle,
  rendererToPaint,
  filterToExpr,
};

function wrapStyleWithFilter(baseStyle, filter) {
  if (!filter || !baseStyle) return baseStyle;

  const isStyleFn = typeof baseStyle === 'function';

  return (feature, resolution) => {
    if (filter && !evalExpr(filter, feature, resolution)) return null;
    return isStyleFn ? baseStyle(feature, resolution) : baseStyle;
  };
}

function composeStyles(symbolStyle, labelStyle) {
  if (!symbolStyle) return labelStyle;
  if (!labelStyle) return symbolStyle;

  const symIsFn = typeof symbolStyle === 'function';
  const lblIsFn = typeof labelStyle === 'function';

  if (!symIsFn && !lblIsFn) return [symbolStyle, labelStyle];

  return (feature, resolution) => {
    const sym = symIsFn ? symbolStyle(feature, resolution) : symbolStyle;
    const lbl = lblIsFn ? labelStyle(feature, resolution) : labelStyle;
    if (!sym && !lbl) return null;
    if (!sym) return lbl;
    if (!lbl) return sym;
    return [sym, lbl];
  };
}

export function styleJsonToOlLayers(styleJson, sourceMeta) {
  if (!styleJson || !styleJson.layers) return [];

  return styleJson.layers.map((layer) => {
    const sourceId = layer.source;
    const geomType = LAYER_TYPE_MAP[layer.type] || 'polygon';

    const renderer = layer['_mapflow:renderer'];
    let olStyle;
    if (renderer) {
      olStyle = rendererToOlStyle(renderer, layer.type);
    } else {
      olStyle = paintToOlStyle(layer.type, layer.paint);
    }

    const labelConfig = layer['_mapflow:label'];
    const labelStyle = labelToOlStyle(labelConfig);
    olStyle = composeStyles(olStyle, labelStyle);

    if (olStyle && layer.filter) {
      olStyle = wrapStyleWithFilter(olStyle, layer.filter);
    }

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
      opacity: renderer?.opacity ?? null,
      minzoom: layer.minzoom,
      maxzoom: layer.maxzoom,
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
      l.id === layerId ? { ...l, paint: { ...(l.paint || {}), ...paintUpdates } } : l,
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
