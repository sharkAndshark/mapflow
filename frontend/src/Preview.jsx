import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useParams, Link } from 'react-router-dom';

import { formatInspectorValue } from './featureInspectorFormat.js';

import 'ol/ol.css';
import OLMap from 'ol/Map';
import View from 'ol/View';
import VectorTileLayer from 'ol/layer/VectorTile';
import VectorTileSource from 'ol/source/VectorTile';
import TileLayer from 'ol/layer/Tile';
import TileDebug from 'ol/source/TileDebug';
import MVT from 'ol/format/MVT';
import { fromLonLat, transformExtent } from 'ol/proj';
import { Fill, Stroke, Style, Circle as CircleStyle } from 'ol/style';

export default function Preview() {
  const { id } = useParams();
  const mapElement = useRef(null);
  const mapRef = useRef(null); // Store the OL map instance
  const vectorLayerRef = useRef(null); // Store the vector tile layer for style updates
  const [meta, setMeta] = useState(null);
  const [error, setError] = useState(null);
  const [selectedFid, setSelectedFid] = useState(null);
  const [popupContent, setPopupContent] = useState(null);
  const [popupLoading, setPopupLoading] = useState(false);
  const [popupError, setPopupError] = useState(null);
  const [popupFid, setPopupFid] = useState(null);
  const popupRef = useRef(null);
  const requestSeqRef = useRef(0);
  const selectedFidRef = useRef(null);
  const [showTileGrid, setShowTileGrid] = useState(false);
  const tileGridLayerRef = useRef(null);

  const cancelPopup = useCallback(() => {
    requestSeqRef.current += 1;
    setPopupContent(null);
    setPopupError(null);
    setPopupLoading(false);
    setPopupFid(null);
    selectedFidRef.current = null;
    setSelectedFid(null);
  }, []);

  useEffect(() => {
    selectedFidRef.current = selectedFid;
  }, [selectedFid]);

  const loadFeatureProperties = useCallback(
    async (fid) => {
      const seq = requestSeqRef.current + 1;
      requestSeqRef.current = seq;

      setPopupFid(fid);
      setPopupLoading(true);
      setPopupError(null);
      setPopupContent(null);
      try {
        const res = await fetch(`/api/files/${id}/features/${fid}`);
        if (!res.ok) {
          let message = 'Failed to load feature properties';
          try {
            const data = await res.json();
            if (data && typeof data.error === 'string') {
              message = data.error;
            }
          } catch (_) {
            // ignore
          }
          throw new Error(message);
        }
        const data = await res.json();
        if (seq !== requestSeqRef.current) {
          return;
        }
        if (!data || !Array.isArray(data.properties)) {
          throw new Error('Invalid feature properties response');
        }
        if (typeof data.fid === 'number') {
          setPopupFid(data.fid);
        }
        setPopupContent(data.properties);
      } catch (e) {
        if (seq !== requestSeqRef.current) {
          return;
        }
        setPopupError(e instanceof Error ? e.message : 'Failed to load feature properties');
        setPopupContent(null);
      } finally {
        if (seq === requestSeqRef.current) {
          setPopupLoading(false);
        }
      }
    },
    [id],
  );

  const defaultStyle = useMemo(
    () =>
      new Style({
        fill: new Fill({
          color: 'rgba(0, 128, 255, 0.6)',
        }),
        stroke: new Stroke({
          color: '#0080ff',
          width: 2,
        }),
        image: new CircleStyle({
          radius: 6,
          fill: new Fill({ color: '#ff0040' }),
          stroke: new Stroke({ color: '#fff', width: 1 }),
        }),
      }),
    [],
  );

  const selectedStyle = useMemo(
    () =>
      new Style({
        fill: new Fill({
          color: 'rgba(255, 200, 0, 0.7)',
        }),
        stroke: new Stroke({
          color: '#ffc800',
          width: 4,
        }),
        image: new CircleStyle({
          radius: 8,
          fill: new Fill({ color: '#ffc800' }),
          stroke: new Stroke({ color: '#fff', width: 2 }),
        }),
      }),
    [],
  );

  const styleFunction = useCallback(
    (feature) => {
      const fid = feature.getId?.() ?? feature.get('fid') ?? feature.getProperties?.()?.fid;
      return fid === selectedFidRef.current ? selectedStyle : defaultStyle;
    },
    [defaultStyle, selectedStyle],
  );

  // Fetch Metadata
  useEffect(() => {
    async function fetchMeta() {
      try {
        const res = await fetch(`/api/files/${id}/preview`);
        if (!res.ok) {
          let message = 'Failed to load preview metadata';
          try {
            const data = await res.json();
            if (data && typeof data.error === 'string') {
              message = data.error;
            }
          } catch (_) {
            // ignore JSON parse errors
          }
          throw new Error(message);
        }
        const data = await res.json();
        setMeta(data);
      } catch (err) {
        setError(err.message);
      }
    }
    fetchMeta();
  }, [id]);

  // Initialize Map
  useEffect(() => {
    if (!mapElement.current || mapRef.current) return;

    const olMap = new OLMap({
      target: mapElement.current,
      view: new View({
        center: fromLonLat([0, 0]),
        zoom: 2,
      }),
      layers: [], // We'll add layers later
    });

    mapRef.current = olMap;

    // Click handler for features
    olMap.on('click', (evt) => {
      const feature = olMap.forEachFeatureAtPixel(evt.pixel, (feature) => feature);
      if (feature) {
        const fid = feature.getId?.() ?? feature.get('fid') ?? feature.getProperties?.()?.fid;
        if (fid === undefined || fid === null || fid === '') {
          setPopupError('Selected feature has no fid');
          setPopupContent(null);
          setPopupLoading(false);
          setPopupFid(null);
          cancelPopup();
          setSelectedFid(null);
          return;
        }

        selectedFidRef.current = fid;
        setSelectedFid(fid);
        // Trigger layer re-render to show highlight immediately
        vectorLayerRef.current?.changed();
        // Load full row properties from DuckDB to ensure stable schema + NULL visibility.
        loadFeatureProperties(fid);
      } else {
        cancelPopup();
        // Trigger layer re-render to clear highlight when clicking empty space
        vectorLayerRef.current?.changed();
      }
    });

    // Add Tile Grid debug layer (initially hidden)
    const tileGridLayer = new TileLayer({
      source: new TileDebug({
        template: 'z:{z} x:{x} y:{y}',
        zDirection: 1,
      }),
      visible: false,
    });
    tileGridLayerRef.current = tileGridLayer;
    olMap.addLayer(tileGridLayer);

    return () => {
      olMap.setTarget(null);
      mapRef.current = null;
      vectorLayerRef.current = null;
      tileGridLayerRef.current = null;
    };
  }, [cancelPopup, loadFeatureProperties]);

  // Update VectorTile Layer and View when Meta changes
  useEffect(() => {
    if (!mapRef.current || !meta) return;

    const map = mapRef.current;

    // Remove existing vector layer only, keep tile grid
    const existingVectorLayer = vectorLayerRef.current;
    if (existingVectorLayer) {
      map.removeLayer(existingVectorLayer);
      vectorLayerRef.current = null;
    }

    // 1. Tile Layer source
    // URL pattern: /api/files/{id}/tiles/{z}/{x}/{y} (no .mvt extension)
    const tileUrl = `${window.location.origin}/api/files/${id}/tiles/{z}/{x}/{y}`;

    const vectorLayer = new VectorTileLayer({
      source: new VectorTileSource({
        format: new MVT(),
        url: tileUrl,
      }),
      style: styleFunction,
    });

    vectorLayerRef.current = vectorLayer;
    // Insert vector layer at index 0, tile grid stays on top
    map.getLayers().insertAt(0, vectorLayer);

    // 2. Fit bounds
    if (meta.bbox && meta.bbox.length === 4) {
      const [minx, miny, maxx, maxy] = meta.bbox;
      // Backend sends WGS84, map is default Web Mercator (EPSG:3857)
      const extent = transformExtent([minx, miny, maxx, maxy], 'EPSG:4326', 'EPSG:3857');

      map.getView().fit(extent, {
        padding: [50, 50, 50, 50],
        duration: 1000,
        maxZoom: 14, // Don't zoom in too close for single points
      });
    }
  }, [meta, id, styleFunction]);

  // Toggle tile grid visibility
  useEffect(() => {
    tileGridLayerRef.current?.setVisible(showTileGrid);
  }, [showTileGrid]);

  return (
    <div
      className="preview-page"
      style={{ height: '100vh', display: 'flex', flexDirection: 'column' }}
    >
      <header
        className="header"
        style={{
          flex: '0 0 auto',
          padding: '16px 24px',
          borderBottom: '1px solid #ececec',
          background: '#fff',
          justifyContent: 'flex-start',
          gap: '16px',
        }}
      >
        <Link to="/" className="back-link">
          ← Back
        </Link>
        {meta && (
          <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
            <h1 style={{ fontSize: '18px', margin: 0 }}>{meta.name}</h1>
            {meta.crs && <span className="badge">{meta.crs}</span>}
          </div>
        )}

        {/* Tile Grid Toggle */}
        <label
          style={{
            marginLeft: 'auto',
            display: 'flex',
            alignItems: 'center',
            gap: '6px',
            fontSize: '13px',
            cursor: 'pointer',
            userSelect: 'none',
          }}
        >
          <input
            type="checkbox"
            checked={showTileGrid}
            onChange={(e) => setShowTileGrid(e.target.checked)}
          />
          Show Tile Grid
        </label>
      </header>

      <div style={{ flex: '1 1 auto', position: 'relative', overflow: 'hidden' }}>
        <div ref={mapElement} style={{ width: '100%', height: '100%', background: '#f5f4f2' }} />

        {/* Loading Overlay */}
        {!meta && !error && (
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
            <p>Loading Map Data...</p>
          </div>
        )}

        {/* Error Overlay */}
        {error && (
          <div
            style={{
              position: 'absolute',
              inset: 0,
              background: 'rgba(255,255,255,0.9)',
              display: 'flex',
              justifyContent: 'center',
              alignItems: 'center',
              zIndex: 20,
            }}
          >
            <div className="alert error-alert">{error}</div>
          </div>
        )}

        {/* Simple Property Inspector Overlay */}
        {(popupContent || popupLoading || popupError) && (
          <div
            style={{
              position: 'absolute',
              top: '20px',
              right: '20px',
              background: 'white',
              padding: '15px',
              borderRadius: '8px',
              boxShadow: '0 4px 12px rgba(0,0,0,0.15)',
              maxWidth: '300px',
              maxHeight: '400px',
              overflow: 'auto',
              zIndex: 100,
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: '10px' }}>
              <h4 style={{ margin: 0 }}>
                Feature Properties
                {popupFid !== null && (
                  <span style={{ marginLeft: '8px', fontSize: '11px', color: '#777' }}>
                    fid: {popupFid}
                  </span>
                )}
              </h4>
              <button
                onClick={cancelPopup}
                type="button"
                style={{ background: 'none', border: 'none', cursor: 'pointer', fontSize: '16px' }}
              >
                ×
              </button>
            </div>

            {popupError && (
              <div className="alert error-alert" style={{ marginBottom: '10px' }}>
                {popupError}
              </div>
            )}

            {popupLoading && <p style={{ margin: 0, fontSize: '12px', color: '#666' }}>Loading…</p>}

            {Array.isArray(popupContent) && (
              <table style={{ fontSize: '12px', width: '100%', borderCollapse: 'collapse' }}>
                <tbody>
                  {popupContent.map((entry) => {
                    const key = entry?.key;
                    const value = entry?.value;
                    const formatted = formatInspectorValue(value);
                    const isNull = formatted.tone === 'null';
                    const isEmptyString = formatted.tone === 'empty';
                    return (
                      <tr key={String(key)} style={{ borderBottom: '1px solid #eee' }}>
                        <td style={{ fontWeight: '600', padding: '4px 8px 4px 0', color: '#555' }}>
                          {key}
                        </td>
                        <td
                          style={{
                            padding: '4px 0',
                            color: isNull || isEmptyString ? '#888' : undefined,
                            fontStyle: isNull ? 'italic' : undefined,
                          }}
                          title={formatted.title}
                        >
                          {formatted.text}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
