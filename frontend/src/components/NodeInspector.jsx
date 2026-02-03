import React from 'react';

function numberOrZero(value) {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function updateArrayToggle(list, value, enabled) {
  if (enabled) {
    if (list.includes(value)) return list;
    return [...list, value];
  }
  return list.filter((item) => item !== value);
}

export function NodeInspector({
  node,
  resources,
  layers,
  resourceMeta,
  onClose,
  onUpdateNode,
  onLayerSourceChange,
  onRequestMetadata,
  onToggleXyzLayer,
  onApplyXyzBounds,
  onApplyXyzCenter,
}) {
  if (!node) return null;

  const isResource = node.type === 'resource';
  const isLayer = node.type === 'layer';
  const isXyz = node.type === 'xyz';

  const resourceId = node.data.source_resource_id;
  const meta = resourceId ? resourceMeta[resourceId] : null;
  const availableFields = meta?.fields || [];

  return (
    <aside className="inspector">
      <div className="inspector__header">
        <div>
          <div className="inspector__eyebrow">Node Inspector</div>
          <div className="inspector__title">{node.data.name || 'Untitled node'}</div>
        </div>
        <button className="button button--ghost" onClick={onClose}>
          Close
        </button>
      </div>

      <div className="inspector__section">
        <div className="inspector__label">Name</div>
        <input
          className="input"
          value={node.data.name || ''}
          onChange={(event) => onUpdateNode(node.id, { name: event.target.value })}
          disabled={isResource}
        />
      </div>

      <div className="inspector__section">
        <div className="inspector__label">Description</div>
        <textarea
          className="textarea"
          rows={3}
          value={node.data.description || ''}
          onChange={(event) => onUpdateNode(node.id, { description: event.target.value })}
          disabled={isResource}
        />
      </div>

      {isResource && (
        <div className="inspector__section">
          <div className="inspector__label">Resource Details</div>
          <div className="inspector__meta">
            <div>SRID: {node.data.srid || 'Unknown'}</div>
            <div>Files: {node.data.file_path?.length || 0}</div>
            <div>Table: {node.data.duckdb_table_name}</div>
          </div>
        </div>
      )}

      {isLayer && (
        <>
          <div className="inspector__section">
            <div className="inspector__label">Source Resource</div>
            <select
              className="input"
              value={node.data.source_resource_id || ''}
              onChange={(event) => onLayerSourceChange(node.id, event.target.value)}
            >
              <option value="">Select resource...</option>
              {resources.map((resource) => (
                <option key={resource.id} value={resource.id}>
                  {resource.data.name || resource.id}
                </option>
              ))}
            </select>
            <div className="inspector__actions">
              <button
                className="button button--ghost"
                onClick={() => resourceId && onRequestMetadata(resourceId)}
                disabled={!resourceId}
              >
                Load Fields
              </button>
            </div>
          </div>

          <div className="inspector__section">
            <div className="inspector__label">Fields</div>
            {availableFields.length > 0 ? (
              <div className="field-grid">
                {availableFields.map((field) => (
                  <label key={field} className="checkbox">
                    <input
                      type="checkbox"
                      checked={node.data.fields?.includes(field) || false}
                      onChange={(event) => {
                        const next = updateArrayToggle(node.data.fields || [], field, event.target.checked);
                        onUpdateNode(node.id, { fields: next });
                      }}
                    />
                    <span>{field}</span>
                  </label>
                ))}
              </div>
            ) : (
              <input
                className="input"
                placeholder="name, type, height"
                value={(node.data.fields || []).join(', ')}
                onChange={(event) => {
                  const next = event.target.value
                    .split(',')
                    .map((item) => item.trim())
                    .filter(Boolean);
                  onUpdateNode(node.id, { fields: next });
                }}
              />
            )}
          </div>

          <div className="inspector__section inspector__grid">
            <label>
              <span className="inspector__label">Min Zoom</span>
              <input
                className="input"
                type="number"
                value={node.data.minzoom ?? 0}
                onChange={(event) => onUpdateNode(node.id, { minzoom: numberOrZero(event.target.value) })}
              />
            </label>
            <label>
              <span className="inspector__label">Max Zoom</span>
              <input
                className="input"
                type="number"
                value={node.data.maxzoom ?? 22}
                onChange={(event) => onUpdateNode(node.id, { maxzoom: numberOrZero(event.target.value) })}
              />
            </label>
          </div>
        </>
      )}

      {isXyz && (
        <>
          <div className="inspector__section inspector__grid">
            <label>
              <span className="inspector__label">Min Zoom</span>
              <input
                className="input"
                type="number"
                value={node.data.min_zoom ?? 0}
                onChange={(event) => onUpdateNode(node.id, { min_zoom: numberOrZero(event.target.value) })}
              />
            </label>
            <label>
              <span className="inspector__label">Max Zoom</span>
              <input
                className="input"
                type="number"
                value={node.data.max_zoom ?? 22}
                onChange={(event) => onUpdateNode(node.id, { max_zoom: numberOrZero(event.target.value) })}
              />
            </label>
            <label>
              <span className="inspector__label">Fill Zoom</span>
              <input
                className="input"
                type="number"
                value={node.data.fillzoom ?? 12}
                onChange={(event) => onUpdateNode(node.id, { fillzoom: numberOrZero(event.target.value) })}
              />
            </label>
          </div>

          <div className="inspector__section">
            <div className="inspector__label">Center (lon, lat, zoom)</div>
            <div className="inspector__grid">
              {(node.data.center || [0, 0, 2]).map((value, index) => (
                <input
                  key={index}
                  className="input"
                  type="number"
                  value={value}
                  onChange={(event) => {
                    const next = [...(node.data.center || [0, 0, 2])];
                    next[index] = numberOrZero(event.target.value);
                    onUpdateNode(node.id, { center: next });
                  }}
                />
              ))}
            </div>
            <div className="inspector__actions">
              <button className="button button--ghost" onClick={() => onApplyXyzCenter(node.id)}>
                Use Resource Center
              </button>
            </div>
          </div>

          <div className="inspector__section">
            <div className="inspector__label">Bounds (minx, miny, maxx, maxy)</div>
            <div className="inspector__grid">
              {(node.data.bounds || [-180, -85.0511, 180, 85.0511]).map((value, index) => (
                <input
                  key={index}
                  className="input"
                  type="number"
                  value={value}
                  onChange={(event) => {
                    const next = [...(node.data.bounds || [-180, -85.0511, 180, 85.0511])];
                    next[index] = numberOrZero(event.target.value);
                    onUpdateNode(node.id, { bounds: next });
                  }}
                />
              ))}
            </div>
            <div className="inspector__actions">
              <button className="button button--ghost" onClick={() => onApplyXyzBounds(node.id)}>
                Use Resource Bounds
              </button>
            </div>
          </div>

          <div className="inspector__section">
            <div className="inspector__label">Layers</div>
            <div className="field-grid">
              {layers.map((layer) => (
                <label key={layer.id} className="checkbox">
                  <input
                    type="checkbox"
                    checked={node.data.layers?.some((ref) => ref.source_layer_id === layer.id) || false}
                    onChange={(event) => onToggleXyzLayer(node.id, layer.id, event.target.checked)}
                  />
                  <span>{layer.data.name || layer.id}</span>
                </label>
              ))}
            </div>
          </div>
        </>
      )}
    </aside>
  );
}
