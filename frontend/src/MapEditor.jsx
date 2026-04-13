import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { getMap, updateMap, listPreviewSources } from './api.js';
import ClassificationWizard from './ClassificationWizard.jsx';
import {
  styleJsonToOlLayers,
  buildEmptyStyleJson,
  addSourceToStyle,
  addLayerToStyle,
  removeLayerFromStyle,
  updateLayerPaint,
  moveLayer,
  setLayerVisibility,
  DEFAULT_PAINT,
} from './styleJsonToOl.js';

import 'ol/ol.css';
import OLMap from 'ol/Map';
import View from 'ol/View';
import VectorTileLayer from 'ol/layer/VectorTile';
import VectorTileSource from 'ol/source/VectorTile';
import MVT from 'ol/format/MVT';
import Projection from 'ol/proj/Projection';
import TileGrid from 'ol/tilegrid/TileGrid';
import { fromLonLat, transformExtent } from 'ol/proj';

function calculateCustomResolutions(dataBounds, maxZoom = 20) {
  const width = dataBounds[2] - dataBounds[0];
  const height = dataBounds[3] - dataBounds[1];
  const maxDim = Math.max(width, height);
  if (maxDim <= 0) return Array.from({ length: maxZoom + 1 }, (_, z) => 1 / Math.pow(2, z));
  return Array.from({ length: maxZoom + 1 }, (_, z) => maxDim / (256 * Math.pow(2, z)));
}

const GEOM_TYPE_HINTS = {
  polygon: 'fill',
  'multi-polygon': 'fill',
  line: 'line',
  'multi-line': 'line',
  point: 'circle',
  'multi-point': 'circle',
};

function guessGeomType(source) {
  const name = (source.name || '').toLowerCase();
  if (name.includes('build') || name.includes('建筑')) return 'polygon';
  if (name.includes('road') || name.includes('路') || name.includes('river') || name.includes('河'))
    return 'line';
  return 'polygon';
}

function fitToDataBounds(map, sourceMeta, styleJson) {
  if (!styleJson?.layers?.length) return;
  const firstSourceId = styleJson.layers[0].source;
  const meta = sourceMeta[firstSourceId];
  if (!meta?.dataBounds) return;
  let bounds = meta.dataBounds;
  if (typeof bounds === 'string') {
    try { bounds = JSON.parse(bounds); } catch { return; }
  }
  if (Array.isArray(bounds) && bounds.length === 4) {
    // ok
  } else if (bounds && typeof bounds === 'object' && bounds.minx != null) {
    bounds = [bounds.minx, bounds.miny, bounds.maxx, bounds.maxy];
  } else {
    return;
  }
  const isCustomCRS = meta.crsType === 'custom';
  const view = map.getView();
  if (isCustomCRS) {
    view.fit(bounds, { padding: [50, 50, 50, 50], duration: 500 });
  } else {
    const extent = transformExtent(bounds, 'EPSG:4326', 'EPSG:3857');
    view.fit(extent, { padding: [50, 50, 50, 50], duration: 500 });
  }
}

export default function MapEditor() {
  const { id } = useParams();
  const navigate = useNavigate();
  const { t } = useTranslation();

  const [mapData, setMapData] = useState(null);
  const [styleJson, setStyleJson] = useState(null);
  const [sources, setSources] = useState([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState('');
  const [isSaving, setIsSaving] = useState(false);
  const [editingLayerId, setEditingLayerId] = useState(null);

  const mapElement = useRef(null);
  const olMapRef = useRef(null);
  const layersRef = useRef({});
  const dirtyRef = useRef(false);
  const saveTimeoutRef = useRef(null);

  const sourceMeta = useMemo(() => {
    const meta = {};
    for (const s of sources) {
      let bounds = null;
      if (s.dataBounds) {
        try {
          const parsed = typeof s.dataBounds === 'string' ? JSON.parse(s.dataBounds) : s.dataBounds;
          if (Array.isArray(parsed) && parsed.length === 4) {
            bounds = parsed;
          } else if (parsed && typeof parsed === 'object' && parsed.minx != null) {
            bounds = [parsed.minx, parsed.miny, parsed.maxx, parsed.maxy];
          }
        } catch {}
      }
      meta[s.id] = { crs: s.crs, crsType: s.crsType, dataBounds: bounds };
    }
    return meta;
  }, [sources]);

  useEffect(() => {
    let cancelled = false;
    async function load() {
      try {
        const [map, srcs] = await Promise.all([getMap(id), listPreviewSources()]);
        if (!cancelled) {
          setMapData(map);
          setStyleJson(map.styleJson ? JSON.parse(map.styleJson) : buildEmptyStyleJson());
          setSources(srcs);
          setError('');
        }
      } catch (err) {
        if (!cancelled) setError(err.message || t('map.loadFailed'));
      } finally {
        if (!cancelled) setIsLoading(false);
      }
    }
    load();
    return () => { cancelled = true; };
  }, [id, t]);

  const debouncedSave = useCallback(
    (newStyle) => {
      dirtyRef.current = true;
      if (saveTimeoutRef.current) clearTimeout(saveTimeoutRef.current);
      saveTimeoutRef.current = setTimeout(async () => {
        if (!dirtyRef.current) return;
        dirtyRef.current = false;
        try {
          setIsSaving(true);
          await updateMap(id, { styleJson: JSON.stringify(newStyle) });
        } catch (err) {
          console.error('Auto-save failed:', err);
        } finally {
          setIsSaving(false);
        }
      }, 1000);
    },
    [id],
  );

  const handleStyleChange = useCallback(
    (newStyle) => {
      setStyleJson(newStyle);
      debouncedSave(newStyle);
    },
    [debouncedSave],
  );

  useEffect(() => {
    if (!mapElement.current) return;
    if (olMapRef.current) {
      olMapRef.current.updateSize();
      return;
    }
    const target = mapElement.current;
    const olMap = new OLMap({
      target,
      view: new View({ center: fromLonLat([0, 0]), zoom: 2 }),
    });
    olMapRef.current = olMap;
    olMap.updateSize();
    return () => {
      olMap.setTarget(null);
      olMapRef.current = null;
      layersRef.current = {};
    };
  }, [isLoading, mapData]);

  useEffect(() => {
    if (!olMapRef.current || !styleJson) return;
    const map = olMapRef.current;

    Object.values(layersRef.current).forEach((l) => map.removeLayer(l));
    layersRef.current = {};

    const olLayers = styleJsonToOlLayers(styleJson, sourceMeta);
    let firstCustomCRS = null;

    for (const layerDef of olLayers) {
      const meta = sourceMeta[layerDef.sourceId];
      let projection = 'EPSG:3857';
      let tileGrid = null;

      if (layerDef.isCustomCRS && layerDef.dataBounds) {
        const [minx, miny, maxx, maxy] = layerDef.dataBounds;
        projection = new Projection({
          code: layerDef.customCRS || 'CUSTOM',
          units: 'm',
          extent: [minx, miny, maxx, maxy],
        });
        const resolutions = calculateCustomResolutions(layerDef.dataBounds, 20);
        tileGrid = new TileGrid({
          extent: [minx, miny, maxx, maxy],
          origin: [minx, maxy],
          resolutions,
          tileSize: 256,
        });
        if (!firstCustomCRS) firstCustomCRS = { projection, dataBounds: layerDef.dataBounds };
      }

      const tileUrl = `${window.location.origin}/api/files/${layerDef.sourceId}/tiles/{z}/{x}/{y}`;
      const vtLayer = new VectorTileLayer({
        source: new VectorTileSource({
          format: new MVT(),
          url: tileUrl,
          projection,
          tileGrid,
        }),
        visible: layerDef.visible,
        style: layerDef.olStyle,
      });

      vtLayer.setZIndex(olLayers.length - olLayers.indexOf(layerDef) + 10);
      layersRef.current[layerDef.id] = vtLayer;
      map.addLayer(vtLayer);
    }

    if (firstCustomCRS) {
      const [minx, miny, maxx, maxy] = firstCustomCRS.dataBounds;
      map.setView(
        new View({
          projection: firstCustomCRS.projection,
          center: [(minx + maxx) / 2, (miny + maxy) / 2],
          zoom: 0,
        }),
      );
    }

    if (olLayers.length > 0) {
      setTimeout(() => {
        map.updateSize();
        fitToDataBounds(map, sourceMeta, styleJson);
      }, 300);
    }
  }, [styleJson, sourceMeta]);

  const usedSourceIds = useMemo(() => {
    if (!styleJson?.sources) return new Set();
    return new Set(Object.keys(styleJson.sources));
  }, [styleJson]);

  function handleAddSource(source) {
    let newStyle = addSourceToStyle(styleJson, source.id, sourceMeta[source.id]);
    const geomType = guessGeomType(source);
    const layerType = GEOM_TYPE_HINTS[geomType] || 'fill';
    const paint = DEFAULT_PAINT[layerType] || {};
    newStyle = addLayerToStyle(newStyle, source.id, layerType, paint);
    handleStyleChange(newStyle);
  }

  function handleRemoveLayer(layerId) {
    const newStyle = removeLayerFromStyle(styleJson, layerId);
    handleStyleChange(newStyle);
    if (editingLayerId === layerId) setEditingLayerId(null);
  }

  function handleLayerPaintChange(layerId, paintUpdates) {
    const newStyle = updateLayerPaint(styleJson, layerId, paintUpdates);
    handleStyleChange(newStyle);
  }

  function handleMoveLayer(layerId, direction) {
    handleStyleChange(moveLayer(styleJson, layerId, direction));
  }

  function handleToggleVisibility(layerId) {
    const layer = styleJson.layers.find((l) => l.id === layerId);
    if (!layer) return;
    const visible = layer.layout?.visibility !== 'visible' || layer.layout?.visibility === undefined;
    handleStyleChange(setLayerVisibility(styleJson, layerId, !visible));
  }

  const editingLayer = useMemo(() => {
    if (!editingLayerId || !styleJson) return null;
    return styleJson.layers.find((l) => l.id === editingLayerId);
  }, [editingLayerId, styleJson]);

  const editingSourceId = editingLayer?.source;

  if (isLoading) {
    return (
      <div style={{ display: 'flex', justifyContent: 'center', alignItems: 'center', height: '100vh' }}>
        {t('common.loading')}
      </div>
    );
  }

  if (error && !mapData) {
    return (
      <div style={{ padding: '24px' }}>
        <div className="alert">{error}</div>
        <button type="button" className="btn-secondary" onClick={() => navigate('/')}>
          {t('common.back')}
        </button>
      </div>
    );
  }

  const availableSources = sources.filter((s) => !usedSourceIds.has(s.id));
  const layers = styleJson?.layers || [];

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100vh' }}>
      <header
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '8px 16px',
          borderBottom: '1px solid #e0e0e0',
          background: '#fafafa',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: '12px' }}>
          <button type="button" className="btn-secondary" style={{ fontSize: '13px' }} onClick={() => navigate('/')}>
            {t('common.back')}
          </button>
          <span style={{ fontWeight: 600, fontSize: '15px' }}>{mapData?.name || ''}</span>
          {isSaving && <span style={{ fontSize: '11px', color: '#888' }}>{t('common.saving')}</span>}
        </div>
      </header>

      <div style={{ display: 'flex', flex: 1, overflow: 'hidden' }}>
        <div
          style={{
            width: '260px',
            minWidth: '260px',
            borderRight: '1px solid #e0e0e0',
            overflow: 'auto',
            display: 'flex',
            flexDirection: 'column',
          }}
        >
          <div style={{ padding: '12px', borderBottom: '1px solid #e0e0e0' }}>
            <div style={{ fontSize: '12px', fontWeight: 600, color: '#555', marginBottom: '8px' }}>
              {t('map.layers')}
            </div>
            {layers.map((layer, idx) => {
              const isSelected = editingLayerId === layer.id;
              const isVisible = layer.layout?.visibility !== 'none';
              return (
                <div
                  key={layer.id}
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: '4px',
                    padding: '6px 4px',
                    marginBottom: '2px',
                    borderRadius: '4px',
                    background: isSelected ? '#e3f2fd' : 'transparent',
                    cursor: 'pointer',
                    fontSize: '13px',
                  }}
                  onClick={() => setEditingLayerId(layer.id)}
                  role="button"
                  tabIndex={0}
                >
                  <button
                    type="button"
                    title={isVisible ? t('map.hideLayer') : t('map.showLayer')}
                    onClick={(e) => { e.stopPropagation(); handleToggleVisibility(layer.id); }}
                    style={{
                      background: 'none',
                      border: 'none',
                      cursor: 'pointer',
                      fontSize: '13px',
                      padding: '0 2px',
                      color: isVisible ? '#333' : '#bbb',
                      width: '20px',
                    }}
                  >
                    {isVisible ? '◉' : '○'}
                  </button>
                  <span
                    style={{
                      flex: 1,
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      whiteSpace: 'nowrap',
                      opacity: isVisible ? 1 : 0.5,
                    }}
                  >
                    {layer.id}
                  </span>
                  <button
                    type="button"
                    title={t('map.layerDown')}
                    onClick={(e) => { e.stopPropagation(); handleMoveLayer(layer.id, 'down'); }}
                    disabled={idx === 0}
                    style={{
                      background: 'none',
                      border: 'none',
                      cursor: idx === 0 ? 'default' : 'pointer',
                      fontSize: '11px',
                      padding: '0 2px',
                      color: idx === 0 ? '#ddd' : '#999',
                    }}
                  >
                    ▲
                  </button>
                  <button
                    type="button"
                    title={t('map.layerUp')}
                    onClick={(e) => { e.stopPropagation(); handleMoveLayer(layer.id, 'up'); }}
                    disabled={idx === layers.length - 1}
                    style={{
                      background: 'none',
                      border: 'none',
                      cursor: idx === layers.length - 1 ? 'default' : 'pointer',
                      fontSize: '11px',
                      padding: '0 2px',
                      color: idx === layers.length - 1 ? '#ddd' : '#999',
                    }}
                  >
                    ▼
                  </button>
                  <button
                    type="button"
                    title={t('common.delete')}
                    onClick={(e) => { e.stopPropagation(); handleRemoveLayer(layer.id); }}
                    style={{
                      background: 'none',
                      border: 'none',
                      color: '#999',
                      cursor: 'pointer',
                      fontSize: '14px',
                      padding: '0 2px',
                    }}
                  >
                    ×
                  </button>
                </div>
              );
            })}
          </div>

          {availableSources.length > 0 && (
            <div style={{ padding: '12px' }}>
              <div style={{ fontSize: '12px', fontWeight: 600, color: '#555', marginBottom: '8px' }}>
                {t('map.addDataSource')}
              </div>
              {availableSources.map((source) => (
                <button
                  key={source.id}
                  type="button"
                  className="btn-text"
                  style={{ display: 'block', fontSize: '12px', marginBottom: '4px', textAlign: 'left', width: '100%' }}
                  onClick={() => handleAddSource(source)}
                >
                  + {source.name}
                </button>
              ))}
            </div>
          )}
        </div>

        <div ref={mapElement} style={{ flex: 1, minHeight: 0, position: 'relative' }} />

        {editingLayer && (
          <div
            style={{
              width: '260px',
              minWidth: '260px',
              borderLeft: '1px solid #e0e0e0',
              overflow: 'auto',
              padding: '12px',
            }}
          >
            <div style={{ fontSize: '12px', fontWeight: 600, color: '#555', marginBottom: '12px' }}>
              {t('map.symbolization')}
            </div>
            <ClassificationWizard
              sourceId={editingSourceId}
              layerType={editingLayer.type}
              paint={editingLayer.paint || {}}
              onPaintChange={(paintUpdates) =>
                handleLayerPaintChange(editingLayer.id, paintUpdates)
              }
            />
          </div>
        )}
      </div>
    </div>
  );
}
