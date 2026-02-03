import React from 'react';
import { Handle, Position } from 'reactflow';

export function XyzNode({ data }) {
  const canOpenTile = Array.isArray(data?.center) && data.center.length >= 2 && Number.isFinite(data.center[2]);

  const handleOpenTile = (event) => {
    event.stopPropagation();
    if (!canOpenTile) return;

    const [lon, lat, zoomRaw] = data.center;
    const z = Math.round(zoomRaw);
    const maxLat = 85.0511;
    const clampedLat = Math.max(Math.min(lat, maxLat), -maxLat);
    const latRad = (clampedLat * Math.PI) / 180;
    const n = 2 ** z;
    const x = Math.floor(((lon + 180) / 360) * n);
    const y = Math.floor((1 - Math.log(Math.tan(latRad) + 1 / Math.cos(latRad)) / Math.PI) / 2 * n);

    const base = window.location.origin;
    window.open(`${base}/tiles/${z}/${x}/${y}.pbf`, '_blank', 'noopener,noreferrer');
  };

  return (
    <div className="node-card node-card--xyz">
      <Handle type="target" position={Position.Left} className="node-handle node-handle--xyz" />
      <div className="node-kicker">XYZ Service</div>
      <div className="node-title">{data.name || 'Untitled XYZ'}</div>
      {data.description && <div className="node-description">{data.description}</div>}
      <div className="node-meta">
        <span>Zoom {data.min_zoom ?? '--'}-{data.max_zoom ?? '--'}</span>
        <span>{data.layers?.length || 0} layers</span>
      </div>
      <div className={`node-status ${data.layers?.length ? 'node-status--ok' : 'node-status--warn'}`}>
        {data.layers?.length ? 'Ready to serve' : 'No layers'}
      </div>
      <button
        type="button"
        className="node-action"
        onClick={handleOpenTile}
        disabled={!canOpenTile}
      >
        Open Tile
      </button>
    </div>
  );
}
