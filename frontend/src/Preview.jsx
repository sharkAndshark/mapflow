import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useParams, Link } from 'react-router-dom';

import 'ol/ol.css';
import OLMap from 'ol/Map';
import View from 'ol/View';
import VectorTileLayer from 'ol/layer/VectorTile';
import VectorTileSource from 'ol/source/VectorTile';
import MVT from 'ol/format/MVT';
import { fromLonLat, transformExtent } from 'ol/proj';
import { Fill, Stroke, Style, Circle as CircleStyle } from 'ol/style';

export default function Preview() {
  const { id } = useParams();
  const mapElement = useRef(null);
  const mapRef = useRef(null); // Store the OL map instance
  const [meta, setMeta] = useState(null);
  const [error, setError] = useState(null);
  const [popupContent, setPopupContent] = useState(null);
  const popupRef = useRef(null);

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
        const properties = feature.getProperties();
        // Remove geometry from properties to avoid cluttering popup
        const { geometry, ...props } = properties;
        setPopupContent(props);
      } else {
        setPopupContent(null);
      }
    });

    return () => {
      olMap.setTarget(null);
      mapRef.current = null;
    };
  }, []);

  // Update Layer and View when Meta changes
  useEffect(() => {
    if (!mapRef.current || !meta) return;

    const map = mapRef.current;

    // Clear existing layers (except maybe a base layer if we added one)
    map.getLayers().clear();

    // 1. Tile Layer source
    // URL pattern: /api/files/{id}/tiles/{z}/{x}/{y} (no .mvt extension)
    const tileUrl = `${window.location.origin}/api/files/${id}/tiles/{z}/{x}/{y}`;

    const vectorLayer = new VectorTileLayer({
      source: new VectorTileSource({
        format: new MVT(),
        url: tileUrl,
      }),
      style: defaultStyle,
    });

    map.addLayer(vectorLayer);

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
  }, [meta, id, defaultStyle]);

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
        {popupContent && (
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
              <h4 style={{ margin: 0 }}>Feature Properties</h4>
              <button
                onClick={() => setPopupContent(null)}
                type="button"
                style={{ background: 'none', border: 'none', cursor: 'pointer', fontSize: '16px' }}
              >
                ×
              </button>
            </div>
            <table style={{ fontSize: '12px', width: '100%', borderCollapse: 'collapse' }}>
              <tbody>
                {Object.entries(popupContent).map(([key, value]) => (
                  <tr key={key} style={{ borderBottom: '1px solid #eee' }}>
                    <td style={{ fontWeight: '600', padding: '4px 8px 4px 0', color: '#555' }}>
                      {key}
                    </td>
                    <td style={{ padding: '4px 0' }}>{String(value)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </div>
  );
}
