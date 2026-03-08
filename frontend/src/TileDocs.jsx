import React, { useState } from 'react';
import { useParams, Link } from 'react-router-dom';

import { PublicTileMap, usePublicTileMeta } from './PublicTileViewer.jsx';

function generateOpenLayersCode(meta, origin) {
  const {
    tileSource,
    crsType,
    dataBounds,
    tileFormat,
    tileUrl: tileUrlPath,
    minZoom,
    maxZoom,
    crs,
  } = meta;
  const tileUrl = `${origin}${tileUrlPath}`;
  const isPmtiles = tileSource === 'pmtiles';
  const isCustomCRS = crsType === 'custom' && dataBounds;
  const isRaster = tileFormat === 'png';

  if (isPmtiles) {
    return `import Map from 'ol/Map.js';
import View from 'ol/View.js';
import VectorTileLayer from 'ol/layer/VectorTile.js';
import WebGLTile from 'ol/layer/WebGLTile.js';
import { fromLonLat } from 'ol/proj.js';
import { PMTilesRasterSource, PMTilesVectorSource } from 'ol-pmtiles';
import { PMTiles, TileType } from 'pmtiles';
import { Fill, Stroke, Style } from 'ol/style.js';

const archiveUrl = '${tileUrl}';

async function init() {
  const archive = new PMTiles(archiveUrl);
  const header = await archive.getHeader();

  let layer;
  if (header.tileType === TileType.Mvt) {
    layer = new VectorTileLayer({
      declutter: true,
      source: new PMTilesVectorSource({ url: archiveUrl, tileSize: 256 }),
      style: new Style({
        fill: new Fill({ color: 'rgba(0, 128, 255, 0.6)' }),
        stroke: new Stroke({ color: '#0080ff', width: 2 }),
      }),
    });
  } else {
    layer = new WebGLTile({
      source: new PMTilesRasterSource({ url: archiveUrl, tileSize: [256, 256] }),
    });
  }

  new Map({
    target: 'map',
    layers: [layer],
    view: new View({
      center: fromLonLat([header.centerLon, header.centerLat]),
      zoom: header.centerZoom,
      minZoom: header.minZoom,
      maxZoom: header.maxZoom,
    }),
  });
}

init();`;
  }

  let code = '';

  if (isCustomCRS) {
    code += `import Map from 'ol/Map.js';
import View from 'ol/View.js';
import ${isRaster ? 'TileLayer' : 'VectorTileLayer'} from 'ol/layer/${isRaster ? 'Tile' : 'VectorTile'}.js';
import ${isRaster ? 'XYZ' : 'VectorTileSource'} from 'ol/source/${isRaster ? 'XYZ' : 'VectorTileSource'}.js';
${isRaster ? '' : "import MVT from 'ol/format/MVT.js';"}
import Projection from 'ol/proj/Projection.js';
import TileGrid from 'ol/tilegrid/TileGrid.js';
${isRaster ? '' : "import { Fill, Stroke, Style } from 'ol/style.js';"}

const dataBounds = [${dataBounds.join(', ')}];
const maxZoom = ${maxZoom ?? 20};

const projection = new Projection({
  code: '${crs || 'CUSTOM_CRS'}',
  units: 'm',
  extent: dataBounds,
});

const resolutions = Array.from(
  { length: maxZoom + 1 },
  (_, z) => (dataBounds[2] - dataBounds[0]) / (256 * Math.pow(2, z))
);

const tileGrid = new TileGrid({
  extent: dataBounds,
  origin: [dataBounds[0], dataBounds[3]],
  resolutions: resolutions,
  tileSize: 256,
});

const map = new Map({
  target: 'map',
  layers: [
    new ${isRaster ? 'TileLayer' : 'VectorTileLayer'}({
      source: new ${isRaster ? 'XYZ' : 'VectorTileSource'}({
        ${isRaster ? '' : 'format: new MVT(),'}
        url: '${tileUrl}',
        projection: projection,
        tileGrid: tileGrid,
      })${
        isRaster
          ? ''
          : `,
      style: new Style({
        fill: new Fill({ color: 'rgba(0, 128, 255, 0.6)' }),
        stroke: new Stroke({ color: '#0080ff', width: 2 }),
      })`
      },
    }),
  ],
  view: new View({
    projection: projection,
    center: [(dataBounds[0] + dataBounds[2]) / 2, (dataBounds[1] + dataBounds[3]) / 2],
    zoom: 0,
    minZoom: ${minZoom ?? 0},
    maxZoom: maxZoom,
  }),
});`;
  } else {
    code += `import Map from 'ol/Map.js';
import View from 'ol/View.js';
import ${isRaster ? 'TileLayer' : 'VectorTileLayer'} from 'ol/layer/${isRaster ? 'Tile' : 'VectorTile'}.js';
import ${isRaster ? 'XYZ' : 'VectorTileSource'} from 'ol/source/${isRaster ? 'XYZ' : 'VectorTileSource'}.js';
${isRaster ? '' : "import MVT from 'ol/format/MVT.js';"}
${isRaster ? '' : "import { Fill, Stroke, Style } from 'ol/style.js';"}
import { fromLonLat } from 'ol/proj.js';

const map = new Map({
  target: 'map',
  layers: [
    new ${isRaster ? 'TileLayer' : 'VectorTileLayer'}({
      source: new ${isRaster ? 'XYZ' : 'VectorTileSource'}({
        ${isRaster ? '' : 'format: new MVT(),'}
        url: '${tileUrl}',
      })${
        isRaster
          ? ''
          : `,
      style: new Style({
        fill: new Fill({ color: 'rgba(0, 128, 255, 0.6)' }),
        stroke: new Stroke({ color: '#0080ff', width: 2 }),
      })`
      },
    }),
  ],
  view: new View({
    center: fromLonLat([0, 0]),
    zoom: 2,
    minZoom: ${minZoom ?? 0},
    maxZoom: ${maxZoom ?? 22},
  }),
});`;
  }

  return code;
}

function generateMarkdownDoc(meta, origin) {
  const {
    slug,
    name,
    tileSource,
    tileUrl,
    viewerUrl,
    crs,
    crsType,
    bbox,
    dataBounds,
    tileFormat,
    minZoom,
    maxZoom,
  } = meta;
  const fullTileUrl = `${origin}${tileUrl}`;
  const fullMetaUrl = `${origin}/tiles/${slug}/meta`;
  const fullViewerUrl = viewerUrl ? `${origin}${viewerUrl}` : null;

  let md = `## Tile Service: ${name}

### Service URLs

| Endpoint | URL |
|----------|-----|
| Tile URL | \`${fullTileUrl}\` |
| Meta API | \`${fullMetaUrl}\` |
| Embed URL | \`${fullViewerUrl}\` |

`;

  md += `

### Configuration

| Property | Value |
|----------|-------|
| Zoom Range | ${minZoom ?? 0} - ${maxZoom ?? 22} |
| CRS | ${crs || 'EPSG:3857'} (${crsType === 'custom' ? 'Custom' : 'Standard'}) |
| Format | ${tileFormat?.toUpperCase() || 'MVT'} |
| Source | ${tileSource} |
`;

  if (crsType === 'custom' && dataBounds) {
    md += `| Data Bounds | [${dataBounds.map((n) => n.toFixed(2)).join(', ')}] |
`;
  }

  if (bbox) {
    md += `| BBox (WGS84) | [${bbox.map((n) => n.toFixed(4)).join(', ')}] |
`;
  }

  md += `
### OpenLayers Example

\`\`\`js
${generateOpenLayersCode(meta, origin)}
\`\`\`
`;

  return md;
}

function CopyButton({ text, label }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy:', err);
    }
  };

  return (
    <button
      type="button"
      onClick={handleCopy}
      className="copy-btn"
      style={{
        padding: '4px 10px',
        fontSize: '12px',
        cursor: 'pointer',
        background: copied ? '#28a745' : '#6c757d',
        color: 'white',
        border: 'none',
        borderRadius: '4px',
        marginLeft: '8px',
      }}
    >
      {copied ? 'Copied!' : label}
    </button>
  );
}

export default function TileDocs() {
  const { slug } = useParams();
  const [showCode, setShowCode] = useState(true);
  const [mdCopied, setMdCopied] = useState(false);
  const { meta, error, isLoading } = usePublicTileMeta(slug);

  const origin = window.location.origin;

  const openLayersCode = meta ? generateOpenLayersCode(meta, origin) : '';
  const markdownDoc = meta ? generateMarkdownDoc(meta, origin) : '';

  return (
    <div
      className="tile-docs-page"
      style={{ height: '100vh', display: 'flex', flexDirection: 'column' }}
    >
      <header
        className="header"
        style={{
          flex: '0 0 auto',
          padding: '12px 24px',
          borderBottom: '1px solid #ececec',
          background: '#fff',
          display: 'flex',
          alignItems: 'center',
          gap: '16px',
        }}
      >
        <Link to="/" className="back-link">
          Back to Files
        </Link>
        {meta && (
          <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
            <h1 style={{ fontSize: '18px', margin: 0 }}>{meta.name}</h1>
            {meta.crsType === 'custom' ? (
              <span className="badge" style={{ backgroundColor: '#f0ad4e', color: '#fff' }}>
                {meta.crs || 'Custom CRS'}
              </span>
            ) : meta.crs ? (
              <span className="badge">{meta.crs}</span>
            ) : null}
            <span
              className="badge"
              style={{ backgroundColor: '#5cb85c', color: '#fff', marginLeft: '4px' }}
            >
              {meta.tileFormat?.toUpperCase() || 'MVT'}
            </span>
          </div>
        )}
      </header>

      <div style={{ flex: '1 1 auto', display: 'flex', overflow: 'hidden' }}>
        <div
          className="docs-panel"
          style={{
            width: '45%',
            minWidth: '400px',
            maxWidth: '600px',
            overflow: 'auto',
            padding: '20px',
            background: '#fafafa',
            borderRight: '1px solid #e0e0e0',
          }}
        >
          {error && (
            <div className="alert error-alert" style={{ marginBottom: '16px' }}>
              {error}
            </div>
          )}

          {isLoading && !meta && !error && (
            <div style={{ textAlign: 'center', padding: '40px' }}>
              <div className="spinner"></div>
              <p>Loading documentation...</p>
            </div>
          )}

          {meta && (
            <>
              <section style={{ marginBottom: '24px' }}>
                <h2
                  style={{
                    fontSize: '16px',
                    marginBottom: '12px',
                    borderBottom: '1px solid #ddd',
                    paddingBottom: '8px',
                  }}
                >
                  Service URLs
                </h2>
                <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
                  <div style={{ display: 'flex', alignItems: 'center' }}>
                    <span style={{ fontWeight: '500', width: '80px' }}>Tile URL</span>
                    <code
                      style={{
                        flex: 1,
                        background: '#e9ecef',
                        padding: '6px 10px',
                        borderRadius: '4px',
                        fontSize: '13px',
                        wordBreak: 'break-all',
                      }}
                    >
                      {origin}
                      {meta.tileUrl}
                    </code>
                    <CopyButton text={`${origin}${meta.tileUrl}`} label="Copy" />
                  </div>
                  <div style={{ display: 'flex', alignItems: 'center' }}>
                    <span style={{ fontWeight: '500', width: '80px' }}>Meta API</span>
                    <code
                      style={{
                        flex: 1,
                        background: '#e9ecef',
                        padding: '6px 10px',
                        borderRadius: '4px',
                        fontSize: '13px',
                        wordBreak: 'break-all',
                      }}
                    >
                      {origin}/tiles/{slug}/meta
                    </code>
                    <CopyButton text={`${origin}/tiles/${slug}/meta`} label="Copy" />
                  </div>
                  <div style={{ display: 'flex', alignItems: 'center' }}>
                    <span style={{ fontWeight: '500', width: '80px' }}>Embed URL</span>
                    <code
                      style={{
                        flex: 1,
                        background: '#e9ecef',
                        padding: '6px 10px',
                        borderRadius: '4px',
                        fontSize: '13px',
                        wordBreak: 'break-all',
                      }}
                    >
                      {origin}
                      {meta.viewerUrl}
                    </code>
                    <CopyButton text={`${origin}${meta.viewerUrl}`} label="Copy" />
                  </div>
                </div>
              </section>

              <section style={{ marginBottom: '24px' }}>
                <h2
                  style={{
                    fontSize: '16px',
                    marginBottom: '12px',
                    borderBottom: '1px solid #ddd',
                    paddingBottom: '8px',
                  }}
                >
                  Configuration
                </h2>
                <table style={{ width: '100%', fontSize: '14px', borderCollapse: 'collapse' }}>
                  <tbody>
                    <tr>
                      <td style={{ padding: '8px 0', fontWeight: '500', width: '120px' }}>
                        Zoom Range
                      </td>
                      <td style={{ padding: '8px 0' }}>
                        {meta.minZoom ?? 0} - {meta.maxZoom ?? 22}
                      </td>
                    </tr>
                    <tr>
                      <td style={{ padding: '8px 0', fontWeight: '500' }}>CRS</td>
                      <td style={{ padding: '8px 0' }}>
                        {meta.crs || 'EPSG:3857'}
                        {meta.crsType === 'custom' && (
                          <span
                            style={{
                              marginLeft: '8px',
                              fontSize: '11px',
                              background: '#f0ad4e',
                              color: '#fff',
                              padding: '2px 6px',
                              borderRadius: '3px',
                            }}
                          >
                            Custom
                          </span>
                        )}
                      </td>
                    </tr>
                    <tr>
                      <td style={{ padding: '8px 0', fontWeight: '500' }}>Format</td>
                      <td style={{ padding: '8px 0' }}>
                        {meta.tileFormat?.toUpperCase() || 'MVT'}
                      </td>
                    </tr>
                    <tr>
                      <td style={{ padding: '8px 0', fontWeight: '500' }}>Source</td>
                      <td style={{ padding: '8px 0' }}>{meta.tileSource}</td>
                    </tr>
                    {meta.crsType === 'custom' && meta.dataBounds && (
                      <tr>
                        <td style={{ padding: '8px 0', fontWeight: '500' }}>Data Bounds</td>
                        <td style={{ padding: '8px 0', fontFamily: 'monospace', fontSize: '12px' }}>
                          [{meta.dataBounds.map((n) => n.toFixed(2)).join(', ')}]
                        </td>
                      </tr>
                    )}
                    {meta.bbox && (
                      <tr>
                        <td style={{ padding: '8px 0', fontWeight: '500' }}>BBox (WGS84)</td>
                        <td style={{ padding: '8px 0', fontFamily: 'monospace', fontSize: '12px' }}>
                          [{meta.bbox.map((n) => n.toFixed(4)).join(', ')}]
                        </td>
                      </tr>
                    )}
                  </tbody>
                </table>
              </section>

              <section style={{ marginBottom: '24px' }}>
                <div
                  style={{
                    display: 'flex',
                    justifyContent: 'space-between',
                    alignItems: 'center',
                    marginBottom: '12px',
                  }}
                >
                  <h2
                    style={{
                      fontSize: '16px',
                      margin: 0,
                      borderBottom: '1px solid #ddd',
                      paddingBottom: '8px',
                      flex: 1,
                    }}
                  >
                    OpenLayers Code
                  </h2>
                  <CopyButton text={openLayersCode} label="Copy Code" />
                </div>
                <button
                  type="button"
                  onClick={() => setShowCode(!showCode)}
                  style={{
                    width: '100%',
                    padding: '8px',
                    marginBottom: '8px',
                    background: '#fff',
                    border: '1px solid #ccc',
                    borderRadius: '4px',
                    cursor: 'pointer',
                    fontSize: '13px',
                  }}
                >
                  {showCode ? 'Hide Code' : 'Show Code'}
                </button>
                {showCode && (
                  <pre
                    style={{
                      background: '#1e1e1e',
                      color: '#d4d4d4',
                      padding: '16px',
                      borderRadius: '6px',
                      fontSize: '12px',
                      overflow: 'auto',
                      maxHeight: '400px',
                      margin: 0,
                    }}
                  >
                    <code>{openLayersCode}</code>
                  </pre>
                )}
              </section>

              <section>
                <button
                  type="button"
                  onClick={async () => {
                    try {
                      await navigator.clipboard.writeText(markdownDoc);
                      setMdCopied(true);
                      setTimeout(() => setMdCopied(false), 2000);
                    } catch (err) {
                      console.error('Failed to copy:', err);
                    }
                  }}
                  style={{
                    width: '100%',
                    padding: '12px',
                    background: mdCopied ? '#28a745' : '#007bff',
                    color: '#fff',
                    border: 'none',
                    borderRadius: '6px',
                    cursor: 'pointer',
                    fontSize: '14px',
                    fontWeight: '500',
                  }}
                >
                  {mdCopied ? 'Copied!' : 'Copy Full Documentation (Markdown)'}
                </button>
              </section>
            </>
          )}
        </div>

        <div style={{ flex: 1, position: 'relative', overflow: 'hidden' }}>
          <PublicTileMap
            meta={meta}
            error={error}
            isLoading={isLoading}
            dataTestId="tile-docs-map"
            overlayLabel="Live Preview (Public Endpoint)"
          />
        </div>
      </div>
    </div>
  );
}
