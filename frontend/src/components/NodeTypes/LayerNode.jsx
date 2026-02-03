import React from 'react';
import { Handle, Position } from 'reactflow';

export function LayerNode({ data }) {
  return (
    <div className="node-card node-card--layer">
      <Handle type="target" position={Position.Left} className="node-handle node-handle--layer" />
      <Handle type="source" position={Position.Right} className="node-handle node-handle--layer" />
      <div className="node-kicker">Layer</div>
      <div className="node-title">{data.name || 'Untitled layer'}</div>
      {data.description && <div className="node-description">{data.description}</div>}
      <div className="node-meta">
        <span>Zoom {data.minzoom ?? '--'}-{data.maxzoom ?? '--'}</span>
        <span>{data.fields?.length || 0} fields</span>
      </div>
      <div className={`node-status ${data.source_resource_id ? 'node-status--ok' : 'node-status--warn'}`}>
        {data.source_resource_id ? 'Resource linked' : 'Missing resource'}
      </div>
    </div>
  );
}
