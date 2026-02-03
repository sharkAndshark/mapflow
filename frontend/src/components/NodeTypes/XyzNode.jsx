import React from 'react';
import { Handle, Position } from 'reactflow';

export function XyzNode({ data }) {
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
    </div>
  );
}
