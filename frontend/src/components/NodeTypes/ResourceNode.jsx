import React from 'react';
import { Handle, Position } from 'reactflow';

export function ResourceNode({ data }) {
  return (
    <div className="node-card node-card--resource">
      <Handle type="source" position={Position.Right} className="node-handle node-handle--resource" />
      <div className="node-kicker">Resource</div>
      <div className="node-title">{data.name || 'Untitled resource'}</div>
      {data.description && <div className="node-description">{data.description}</div>}
      <div className="node-meta">
        <span>SRID {data.srid || 'unknown'}</span>
        <span>{data.file_path?.length || 0} files</span>
      </div>
    </div>
  );
}
