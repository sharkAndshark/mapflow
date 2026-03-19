import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

import 'ol/ol.css';
import MVT from 'ol/format/MVT';
import OLMap from 'ol/Map';
import TileLayer from 'ol/layer/Tile';
import VectorTileLayer from 'ol/layer/VectorTile';
import WebGLTile from 'ol/layer/WebGLTile';
import { fromLonLat, transformExtent } from 'ol/proj';
import Projection from 'ol/proj/Projection';
import XYZ from 'ol/source/XYZ';
import VectorTileSource from 'ol/source/VectorTile';
import { Circle as CircleStyle, Fill, Stroke, Style } from 'ol/style';
import TileGrid from 'ol/tilegrid/TileGrid';
import View from 'ol/View';
import { PMTilesRasterSource, PMTilesVectorSource } from 'ol-pmtiles';
import { PMTiles, TileType } from 'pmtiles';

function calculateCustomResolutions(dataBounds, maxZoom = 22) {
  const width = dataBounds[2] - dataBounds[0];
  const height = dataBounds[3] - dataBounds[1];
  const maxDimension = Math.max(width, height);

  if (maxDimension <= 0) {
    console.warn('Invalid data bounds: zero or negative extent');
    return Array.from({ length: maxZoom + 1 }, (_, zoom) => 1 / Math.pow(2, zoom));
  }

  return Array.from({ length: maxZoom + 1 }, (_, zoom) => maxDimension / (256 * Math.pow(2, zoom)));
}

function normalizeZoom(value, fallback) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    return fallback;
  }

  return Math.max(0, Math.floor(parsed));
}

function clampZoom(value, minZoom, maxZoom) {
  return Math.min(maxZoom, Math.max(minZoom, value));
}

function resolvePmtilesViewZoomRange(meta, header) {
  const headerMinZoom = normalizeZoom(header.minZoom, 0);
  const headerMaxZoom = Math.max(headerMinZoom, normalizeZoom(header.maxZoom, 22));

  const publishedMinZoom =
    meta.minZoom == null
      ? null
      : clampZoom(normalizeZoom(meta.minZoom, headerMinZoom), headerMinZoom, headerMaxZoom);
  const publishedMaxZoom =
    meta.maxZoom == null
      ? null
      : clampZoom(normalizeZoom(meta.maxZoom, headerMaxZoom), headerMinZoom, headerMaxZoom);

  let viewMinZoom = publishedMinZoom ?? headerMinZoom;
  let viewMaxZoom = publishedMaxZoom ?? headerMaxZoom;

  if (viewMinZoom > viewMaxZoom) {
    viewMinZoom = headerMinZoom;
    viewMaxZoom = headerMaxZoom;
  }

  return { viewMinZoom, viewMaxZoom };
}

export function usePublicTileMeta(slug) {
  const { t } = useTranslation();
  const [meta, setMeta] = useState(null);
  const [error, setError] = useState(null);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    if (!slug) {
      setMeta(null);
      setError(t('errors.missingTileSlug'));
      setIsLoading(false);
      return;
    }

    let cancelled = false;
    setMeta(null);
    setError(null);
    setIsLoading(true);

    async function fetchMeta() {
      try {
        const response = await fetch(`/tiles/${slug}/meta`);
        if (!response.ok) {
          let message = t('errors.loadTileMetaFailed');
          try {
            const data = await response.json();
            if (data && typeof data.error === 'string') {
              message = data.error;
            }
          } catch {
            // Ignore JSON parsing errors and keep the fallback message.
          }
          throw new Error(message);
        }

        const data = await response.json();
        if (!cancelled) {
          setMeta(data);
        }
      } catch (fetchError) {
        if (!cancelled) {
          setError(fetchError.message);
        }
      } finally {
        if (!cancelled) {
          setIsLoading(false);
        }
      }
    }

    fetchMeta();

    return () => {
      cancelled = true;
    };
  }, [slug, t]);

  return { meta, error, isLoading };
}

export function PublicTileMap({
  meta,
  error = null,
  isLoading = false,
  overlayLabel,
  dataTestId = 'public-tile-map',
  exposeMapGlobally = false,
  style,
}) {
  const { t } = useTranslation();
  const mapElement = useRef(null);
  const mapRef = useRef(null);
  const tileLayerRef = useRef(null);
  const [runtimeMessage, setRuntimeMessage] = useState(null);
  const [isArchiveLoading, setIsArchiveLoading] = useState(false);

  const defaultStyle = useMemo(
    () =>
      new Style({
        fill: new Fill({ color: 'rgba(0, 128, 255, 0.6)' }),
        stroke: new Stroke({ color: '#0080ff', width: 2 }),
        image: new CircleStyle({
          radius: 6,
          fill: new Fill({ color: '#ff0040' }),
          stroke: new Stroke({ color: '#fff', width: 1 }),
        }),
      }),
    [],
  );

  useEffect(() => {
    if (!mapElement.current || mapRef.current) {
      return undefined;
    }

    const targetElement = mapElement.current;
    const olMap = new OLMap({
      target: targetElement,
      view: new View({
        projection: 'EPSG:3857',
        center: [0, 0],
        zoom: 2,
        minZoom: 0,
        maxZoom: 22,
      }),
      layers: [],
    });

    mapRef.current = olMap;

    if (exposeMapGlobally) {
      targetElement.__mapflowPublicTileMap = olMap;
      window.__mapflowPublicTileMap = olMap;
    }

    return () => {
      olMap.setTarget(null);

      if (targetElement.__mapflowPublicTileMap === olMap) {
        delete targetElement.__mapflowPublicTileMap;
      }
      if (window.__mapflowPublicTileMap === olMap) {
        delete window.__mapflowPublicTileMap;
      }

      mapRef.current = null;
      tileLayerRef.current = null;
    };
  }, [exposeMapGlobally]);

  useEffect(() => {
    if (!mapRef.current) {
      return undefined;
    }

    let cancelled = false;
    const map = mapRef.current;
    async function syncLayer() {
      const existingLayer = tileLayerRef.current;

      if (existingLayer && (!meta || error)) {
        map.removeLayer(existingLayer);
        tileLayerRef.current = null;
      }

      if (!meta || error) {
        if (!cancelled) {
          setRuntimeMessage(null);
          setIsArchiveLoading(false);
        }
        return;
      }

      const isPmtiles = meta.tileSource === 'pmtiles';
      const isCustomCrs = meta.crsType === 'custom' && Array.isArray(meta.dataBounds);
      const tileFormat = meta.tileFormat || 'mvt';
      const minZoom = normalizeZoom(meta.minZoom, 0);
      const maxZoom = normalizeZoom(meta.maxZoom, 22);
      const customGridMaxZoom = Math.max(0, Math.floor(maxZoom));

      if (existingLayer) {
        map.removeLayer(existingLayer);
        tileLayerRef.current = null;
      }

      if (isPmtiles) {
        if (!cancelled) {
          setRuntimeMessage(null);
          setIsArchiveLoading(true);
        }

        try {
          const archive = new PMTiles(meta.tileUrl);
          const header = await archive.getHeader();
          if (cancelled) {
            return;
          }

          let pmtilesLayer;
          if (header.tileType === TileType.Mvt) {
            pmtilesLayer = new VectorTileLayer({
              declutter: true,
              source: new PMTilesVectorSource({
                url: meta.tileUrl,
              }),
              style: defaultStyle,
            });
          } else if (
            header.tileType === TileType.Png ||
            header.tileType === TileType.Jpeg ||
            header.tileType === TileType.Webp ||
            header.tileType === TileType.Avif
          ) {
            pmtilesLayer = new WebGLTile({
              source: new PMTilesRasterSource({
                url: meta.tileUrl,
                tileSize: [256, 256],
              }),
            });
          } else {
            throw new Error(`Unsupported PMTiles tile type: ${header.tileType}`);
          }

          tileLayerRef.current = pmtilesLayer;
          map.getLayers().insertAt(0, pmtilesLayer);

          const { viewMinZoom, viewMaxZoom } = resolvePmtilesViewZoomRange(meta, header);
          const centerZoom = header.centerZoom ?? viewMinZoom;
          const initialZoom = Math.min(viewMaxZoom, Math.max(viewMinZoom, centerZoom));

          const view = new View({
            projection: 'EPSG:3857',
            center: fromLonLat([header.centerLon, header.centerLat]),
            zoom: initialZoom,
            minZoom: viewMinZoom,
            maxZoom: viewMaxZoom,
            constrainResolution: true,
            smoothResolutionConstraint: false,
          });
          map.setView(view);

          if (
            Number.isFinite(header.minLon) &&
            Number.isFinite(header.minLat) &&
            Number.isFinite(header.maxLon) &&
            Number.isFinite(header.maxLat) &&
            header.minLon < header.maxLon &&
            header.minLat < header.maxLat
          ) {
            const extent = transformExtent(
              [header.minLon, header.minLat, header.maxLon, header.maxLat],
              'EPSG:4326',
              'EPSG:3857',
            );

            view.fit(extent, {
              padding: [40, 40, 40, 40],
              duration: 0,
              maxZoom: viewMaxZoom,
            });
          }

          const fittedZoom = view.getZoom();
          if (fittedZoom != null) {
            const clampedZoom = Math.min(viewMaxZoom, Math.max(viewMinZoom, fittedZoom));
            const clampedResolution = view.getResolutionForZoom(clampedZoom);
            if (clampedResolution != null) {
              view.setResolution(clampedResolution);
            }
          }

          if (!cancelled) {
            setRuntimeMessage(null);
            setIsArchiveLoading(false);
          }
        } catch (loadError) {
          if (!cancelled) {
            setRuntimeMessage(
              loadError instanceof Error ? loadError.message : t('errors.loadPmtilesFailed'),
            );
            setIsArchiveLoading(false);
          }
        }

        return;
      }

      if (!cancelled) {
        setRuntimeMessage(null);
        setIsArchiveLoading(false);
      }

      let customProjection = null;
      let customTileGrid = null;

      if (isCustomCrs) {
        const [minX, minY, maxX, maxY] = meta.dataBounds;

        customProjection = new Projection({
          code: meta.crs || 'CUSTOM_CRS',
          units: 'm',
          extent: [minX, minY, maxX, maxY],
        });

        customTileGrid = new TileGrid({
          extent: [minX, minY, maxX, maxY],
          origin: [minX, maxY],
          resolutions: calculateCustomResolutions(meta.dataBounds, customGridMaxZoom),
          tileSize: 256,
        });
      }

      const tileUrl = meta.tileUrl;
      const layerSourceConfig = {
        url: tileUrl,
        projection: customProjection || 'EPSG:3857',
        tileGrid: customTileGrid,
      };

      const tileLayer =
        tileFormat === 'png'
          ? new TileLayer({
              source: new XYZ(layerSourceConfig),
            })
          : new VectorTileLayer({
              source: new VectorTileSource({
                ...layerSourceConfig,
                format: new MVT(),
              }),
              style: defaultStyle,
            });

      tileLayerRef.current = tileLayer;
      map.getLayers().insertAt(0, tileLayer);

      if (isCustomCrs && customProjection) {
        const [minX, minY, maxX, maxY] = meta.dataBounds;
        map.setView(
          new View({
            projection: customProjection,
            center: [(minX + maxX) / 2, (minY + maxY) / 2],
            zoom: 0,
            minZoom,
            maxZoom,
          }),
        );
      } else {
        map.setView(
          new View({
            projection: 'EPSG:3857',
            center: [0, 0],
            zoom: 2,
            minZoom,
            maxZoom,
          }),
        );
      }

      if (Array.isArray(meta.bbox) && meta.bbox.length === 4) {
        const extent = isCustomCrs
          ? meta.bbox
          : transformExtent(meta.bbox, 'EPSG:4326', 'EPSG:3857');

        map.getView().fit(extent, {
          padding: [40, 40, 40, 40],
          duration: 1000,
          maxZoom,
        });
      }
    }

    syncLayer();

    return () => {
      cancelled = true;
    };
  }, [defaultStyle, error, meta, t]);

  const displayError = error || runtimeMessage;

  return (
    <div style={{ position: 'relative', width: '100%', height: '100%', ...style }}>
      <div
        ref={mapElement}
        data-testid={dataTestId}
        style={{ width: '100%', height: '100%', background: '#f5f4f2' }}
      />

      {((isLoading && !meta) || isArchiveLoading) && !displayError && (
        <div
          style={{
            position: 'absolute',
            inset: 0,
            background: 'rgba(255,255,255,0.8)',
            display: 'flex',
            justifyContent: 'center',
            alignItems: 'center',
            flexDirection: 'column',
            gap: '10px',
            zIndex: 10,
          }}
        >
          <div className="spinner"></div>
          <p>{t('preview.loadingMap')}</p>
        </div>
      )}

      {displayError && (
        <div
          style={{
            position: 'absolute',
            inset: 0,
            background: 'rgba(255,255,255,0.9)',
            display: 'flex',
            justifyContent: 'center',
            alignItems: 'center',
            padding: '24px',
            zIndex: 20,
          }}
        >
          <div className="alert error-alert">{displayError}</div>
        </div>
      )}

      {overlayLabel ? (
        <div
          data-testid="public-tile-overlay-label"
          style={{
            position: 'absolute',
            top: '12px',
            right: '12px',
            background: 'rgba(255,255,255,0.92)',
            padding: '8px 12px',
            borderRadius: '4px',
            fontSize: '12px',
            color: '#666',
            boxShadow: '0 2px 6px rgba(0,0,0,0.1)',
          }}
        >
          {overlayLabel}
        </div>
      ) : null}
    </div>
  );
}
