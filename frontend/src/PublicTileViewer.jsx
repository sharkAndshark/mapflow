import React, { useEffect, useMemo, useRef, useState } from 'react';

import 'ol/ol.css';
import MVT from 'ol/format/MVT';
import OLMap from 'ol/Map';
import TileLayer from 'ol/layer/Tile';
import VectorTileLayer from 'ol/layer/VectorTile';
import { transformExtent } from 'ol/proj';
import Projection from 'ol/proj/Projection';
import XYZ from 'ol/source/XYZ';
import VectorTileSource from 'ol/source/VectorTile';
import { Circle as CircleStyle, Fill, Stroke, Style } from 'ol/style';
import TileGrid from 'ol/tilegrid/TileGrid';
import View from 'ol/View';

function calculateCustomResolutions(dataBounds, maxZoom = 20) {
  const width = dataBounds[2] - dataBounds[0];
  const height = dataBounds[3] - dataBounds[1];
  const maxDimension = Math.max(width, height);

  if (maxDimension <= 0) {
    console.warn('Invalid data bounds: zero or negative extent');
    return Array.from({ length: maxZoom + 1 }, (_, zoom) => 1 / Math.pow(2, zoom));
  }

  return Array.from({ length: maxZoom + 1 }, (_, zoom) => maxDimension / (256 * Math.pow(2, zoom)));
}

export function usePublicTileMeta(slug) {
  const [meta, setMeta] = useState(null);
  const [error, setError] = useState(null);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    if (!slug) {
      setMeta(null);
      setError('Missing tile slug');
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
          let message = 'Failed to load tile metadata';
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
  }, [slug]);

  return { meta, error, isLoading };
}

export function PublicTileMap({
  meta,
  error = null,
  isLoading = false,
  overlayLabel = 'Live Preview (Public Endpoint)',
  dataTestId = 'public-tile-map',
  exposeMapGlobally = false,
  style,
}) {
  const mapElement = useRef(null);
  const mapRef = useRef(null);
  const tileLayerRef = useRef(null);

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
      return;
    }

    const map = mapRef.current;
    const existingLayer = tileLayerRef.current;

    if (existingLayer && (!meta || error)) {
      map.removeLayer(existingLayer);
      tileLayerRef.current = null;
    }

    if (!meta || error) {
      return;
    }

    const isPmtiles = meta.tileSource === 'pmtiles';
    const isCustomCrs = meta.crsType === 'custom' && Array.isArray(meta.dataBounds);
    const tileFormat = meta.tileFormat || 'mvt';
    const minZoom = meta.minZoom ?? 0;
    const maxZoom = isCustomCrs ? 20 : (meta.maxZoom ?? 22);

    if (existingLayer) {
      map.removeLayer(existingLayer);
      tileLayerRef.current = null;
    }

    if (isPmtiles) {
      map.setView(
        new View({
          projection: 'EPSG:3857',
          center: [0, 0],
          zoom: 2,
          minZoom: 0,
          maxZoom: 22,
        }),
      );
      return;
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
        resolutions: calculateCustomResolutions(meta.dataBounds, 20),
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
      const extent = isCustomCrs ? meta.bbox : transformExtent(meta.bbox, 'EPSG:4326', 'EPSG:3857');

      map.getView().fit(extent, {
        padding: [40, 40, 40, 40],
        duration: 1000,
        maxZoom,
      });
    }
  }, [defaultStyle, error, meta]);

  const isPmtiles = meta?.tileSource === 'pmtiles';

  return (
    <div style={{ position: 'relative', width: '100%', height: '100%', ...style }}>
      <div
        ref={mapElement}
        data-testid={dataTestId}
        style={{ width: '100%', height: '100%', background: '#f5f4f2' }}
      />

      {isLoading && !meta && !error && (
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
          <p>Loading map...</p>
        </div>
      )}

      {isPmtiles && !error && (
        <div
          style={{
            position: 'absolute',
            inset: 0,
            background: 'rgba(255,255,255,0.92)',
            display: 'flex',
            justifyContent: 'center',
            alignItems: 'center',
            textAlign: 'center',
            padding: '24px',
            zIndex: 15,
          }}
        >
          <div style={{ maxWidth: '440px', color: '#444', lineHeight: 1.5 }}>
            <strong>PMTiles embed preview is not available in this page.</strong>
            <p style={{ marginTop: '8px', marginBottom: 0 }}>
              This dataset is exposed as a PMTiles byte-range endpoint. Use a PMTiles-aware client
              if you need a live preview.
            </p>
          </div>
        </div>
      )}

      {error && (
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
          <div className="alert error-alert">{error}</div>
        </div>
      )}

      {overlayLabel ? (
        <div
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
