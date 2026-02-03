import React, { useCallback, useEffect, useMemo, useState } from 'react';
import ReactFlow, {
  Background,
  Controls,
  MiniMap,
  ReactFlowProvider,
  addEdge,
  MarkerType,
  useEdgesState,
  useNodesState,
  useNodesInitialized,
  useReactFlow,
} from 'reactflow';
import 'reactflow/dist/style.css';
import './App.css';

import { ResourceNode } from './components/NodeTypes/ResourceNode';
import { LayerNode } from './components/NodeTypes/LayerNode';
import { XyzNode } from './components/NodeTypes/XyzNode';
import { NodeInspector } from './components/NodeInspector';
import { api } from './api';

const nodeTypes = {
  resource: ResourceNode,
  layer: LayerNode,
  xyz: XyzNode,
};

const DEFAULT_BOUNDS = [-180, -85.0511, 180, 85.0511];
const DEFAULT_CENTER = [0, 0, 2];

const TYPE_COLUMNS = {
  resource: 120,
  layer: 420,
  xyz: 720,
};

function generateId(prefix) {
  if (typeof crypto !== 'undefined' && crypto.randomUUID) {
    return `${prefix}_${crypto.randomUUID().replace(/-/g, '').toUpperCase()}`;
  }
  return `${prefix}_${Math.random().toString(16).slice(2).toUpperCase()}`;
}

function styledEdge(edge) {
  return {
    ...edge,
    type: 'smoothstep',
    markerEnd: { type: MarkerType.ArrowClosed },
    style: { stroke: '#64748b', strokeWidth: 2 },
  };
}

function layoutNodes(configNodes) {
  const counts = { resource: 0, layer: 0, xyz: 0 };
  return configNodes.map((node) => {
    const type = node.type;
    const y = 120 + counts[type] * 180;
    const x = TYPE_COLUMNS[type] ?? 120;
    counts[type] += 1;

    return {
      id: node.id,
      type,
      position: { x, y },
      data: { ...node },
    };
  });
}

function AppShell() {
  const reactFlow = useReactFlow();
  const nodesInitialized = useNodesInitialized();
  const [nodes, setNodes, onNodesChange] = useNodesState([]);
  const [edges, setEdges, onEdgesChange] = useEdgesState([]);
  const [selectedNodeId, setSelectedNodeId] = useState(null);
  const [inspectorOpen, setInspectorOpen] = useState(false);
  const [contextMenu, setContextMenu] = useState(null);
  const [resourceMeta, setResourceMeta] = useState({});
  const [uploadSrid, setUploadSrid] = useState('');
  const [isValidating, setIsValidating] = useState(false);
  const [isApplying, setIsApplying] = useState(false);
  const [validationErrors, setValidationErrors] = useState([]);
  const [showSuccess, setShowSuccess] = useState(false);
  const [statusMessage, setStatusMessage] = useState('');

  const nodeById = useMemo(() => {
    const map = new Map();
    nodes.forEach((node) => map.set(node.id, node));
    return map;
  }, [nodes]);

  const resourceNodes = useMemo(
    () => nodes.filter((node) => node.type === 'resource'),
    [nodes]
  );
  const layerNodes = useMemo(() => nodes.filter((node) => node.type === 'layer'), [nodes]);

  const selectedNode = nodes.find((node) => node.id === selectedNodeId) || null;

  useEffect(() => {
    let active = true;
    api
      .getConfig()
      .then((config) => {
        if (!active) return;
        const incomingNodes = layoutNodes(config.nodes || []);
        const incomingEdges = (config.edges || []).map((edge) =>
          styledEdge({ id: edge.id || generateId('EDGE'), source: edge.source, target: edge.target })
        );
        setNodes(incomingNodes);
        setEdges(incomingEdges);
      })
      .catch((error) => {
        console.error('Failed to load config', error);
        setStatusMessage('Failed to load config. Please refresh.');
      });

    return () => {
      active = false;
    };
  }, [setNodes, setEdges]);

  useEffect(() => {
    setValidationErrors([]);
    setShowSuccess(false);
  }, [nodes, edges]);

  const screenToFlow = useCallback(
    (event) => {
      const bounds = event.currentTarget.getBoundingClientRect();
      const point = { x: event.clientX - bounds.left, y: event.clientY - bounds.top };
      if (reactFlow.screenToFlowPosition) {
        return reactFlow.screenToFlowPosition(point);
      }
      return reactFlow.project(point);
    },
    [reactFlow]
  );

  const updateNodeData = useCallback(
    (id, patch) => {
      setNodes((nds) =>
        nds.map((node) =>
          node.id === id ? { ...node, data: { ...node.data, ...patch } } : node
        )
      );
    },
    [setNodes]
  );

  const syncLayerSourceEdge = useCallback(
    (layerId, resourceId) => {
      setEdges((eds) => {
        const filtered = eds.filter(
          (edge) => !(edge.target === layerId && nodeById.get(edge.source)?.type === 'resource')
        );
        if (!resourceId) return filtered;
        const next = styledEdge({
          id: generateId('EDGE'),
          source: resourceId,
          target: layerId,
        });
        return [...filtered, next];
      });
      updateNodeData(layerId, { source_resource_id: resourceId });
    },
    [nodeById, setEdges, updateNodeData]
  );

  const toggleXyzLayer = useCallback(
    (xyzId, layerId, enabled) => {
      setEdges((eds) => {
        const exists = eds.some((edge) => edge.source === layerId && edge.target === xyzId);
        if (enabled && !exists) {
          return [...eds, styledEdge({ id: generateId('EDGE'), source: layerId, target: xyzId })];
        }
        if (!enabled && exists) {
          return eds.filter((edge) => !(edge.source === layerId && edge.target === xyzId));
        }
        return eds;
      });

      updateNodeData(xyzId, {
        layers: (() => {
          const current = nodeById.get(xyzId)?.data.layers || [];
          if (enabled) {
            if (current.some((ref) => ref.source_layer_id === layerId)) return current;
            return [...current, { id: `layer_${layerId}`, source_layer_id: layerId }];
          }
          return current.filter((ref) => ref.source_layer_id !== layerId);
        })(),
      });
    },
    [nodeById, setEdges, updateNodeData]
  );

  const isValidConnection = useCallback(
    (connection) => {
      const sourceType = nodeById.get(connection.source)?.type;
      const targetType = nodeById.get(connection.target)?.type;
      return (
        (sourceType === 'resource' && targetType === 'layer') ||
        (sourceType === 'layer' && targetType === 'xyz')
      );
    },
    [nodeById]
  );

  const onConnect = useCallback(
    (connection) => {
      if (!isValidConnection(connection)) {
        setStatusMessage('Invalid connection. Allowed: Resource → Layer → XYZ.');
        return;
      }

      if (connection.source && connection.target) {
        const sourceType = nodeById.get(connection.source)?.type;
        const targetType = nodeById.get(connection.target)?.type;

        if (sourceType === 'resource' && targetType === 'layer') {
          syncLayerSourceEdge(connection.target, connection.source);
          return;
        }

        if (sourceType === 'layer' && targetType === 'xyz') {
          toggleXyzLayer(connection.target, connection.source, true);
          return;
        }
      }

      setEdges((eds) => addEdge(styledEdge(connection), eds));
    },
    [isValidConnection, nodeById, setEdges, syncLayerSourceEdge, toggleXyzLayer]
  );

  const onEdgesDelete = useCallback(
    (edgesToDelete) => {
      edgesToDelete.forEach((edge) => {
        const sourceType = nodeById.get(edge.source)?.type;
        const targetType = nodeById.get(edge.target)?.type;

        if (sourceType === 'resource' && targetType === 'layer') {
          updateNodeData(edge.target, { source_resource_id: '' });
        }

        if (sourceType === 'layer' && targetType === 'xyz') {
          updateNodeData(edge.target, {
            layers: (nodeById.get(edge.target)?.data.layers || []).filter(
              (ref) => ref.source_layer_id !== edge.source
            ),
          });
        }
      });
    },
    [nodeById, updateNodeData]
  );

  const onNodesDelete = useCallback(
    (nodesToDelete) => {
      if (nodesToDelete.some((node) => node.id === selectedNodeId)) {
        setSelectedNodeId(null);
        setInspectorOpen(false);
      }
    },
    [selectedNodeId]
  );

  const onNodeDoubleClick = useCallback((_, node) => {
    setSelectedNodeId(node.id);
    setInspectorOpen(true);
  }, []);

  const onPaneContextMenu = useCallback(
    (event) => {
      event.preventDefault();
      const flowPoint = screenToFlow(event);
      setContextMenu({
        x: event.clientX,
        y: event.clientY,
        flowX: flowPoint.x,
        flowY: flowPoint.y,
      });
    },
    [screenToFlow]
  );

  const onPaneClick = useCallback(() => {
    setContextMenu(null);
  }, []);

  const createNode = useCallback(
    (type, position) => {
      const idPrefix = type === 'layer' ? 'LAYER' : 'XYZ';
      const id = generateId(idPrefix);
      const base = {
        id,
        type,
        name: type === 'layer' ? 'new_layer' : 'new_xyz',
        description: '',
        readonly: false,
      };

      const data =
        type === 'layer'
          ? {
              ...base,
              source_resource_id: '',
              fields: [],
              minzoom: 8,
              maxzoom: 14,
            }
          : {
              ...base,
              center: [...DEFAULT_CENTER],
              min_zoom: 0,
              max_zoom: 22,
              fillzoom: 12,
              bounds: [...DEFAULT_BOUNDS],
              layers: [],
            };

      setNodes((nds) => [
        ...nds,
        {
          id,
          type,
          position,
          data,
        },
      ]);
      setSelectedNodeId(id);
      setInspectorOpen(true);
    },
    [setNodes]
  );

  const handleContextCreate = useCallback(
    (type) => {
      if (!contextMenu) return;
      createNode(type, { x: contextMenu.flowX, y: contextMenu.flowY });
      setContextMenu(null);
    },
    [contextMenu, createNode]
  );

  const buildConfig = useCallback(() => {
    const nodeLookup = new Map(nodes.map((node) => [node.id, node]));

    const serializedNodes = nodes.map((node) => {
      const data = node.data || {};
      const base = {
        id: node.id,
        type: node.type,
        name: data.name || '',
        description: data.description || undefined,
        readonly: data.readonly ?? node.type === 'resource',
      };

      if (node.type === 'resource') {
        return {
          ...base,
          resource_type: data.resource_type || 'shapefile',
          file_path: data.file_path || [],
          size: data.size || 0,
          create_timestamp: data.create_timestamp || 0,
          hash: data.hash || '',
          srid: data.srid || '',
          duckdb_table_name: data.duckdb_table_name || '',
        };
      }

      if (node.type === 'layer') {
        const incomingResource = edges.find(
          (edge) => edge.target === node.id && nodeLookup.get(edge.source)?.type === 'resource'
        );

        return {
          ...base,
          source_resource_id: incomingResource?.source || data.source_resource_id || '',
          fields: data.fields?.length ? data.fields : undefined,
          minzoom: data.minzoom ?? 0,
          maxzoom: data.maxzoom ?? 22,
        };
      }

      const incomingLayers = edges.filter(
        (edge) => edge.target === node.id && nodeLookup.get(edge.source)?.type === 'layer'
      );

      const layerRefs = incomingLayers.map((edge) => {
        const existing = data.layers?.find((ref) => ref.source_layer_id === edge.source);
        return existing || { id: `layer_${edge.source}`, source_layer_id: edge.source };
      });

      const center =
        Array.isArray(data.center) && data.center.length === 3 ? data.center : [...DEFAULT_CENTER];
      const bounds =
        Array.isArray(data.bounds) && data.bounds.length === 4 ? data.bounds : [...DEFAULT_BOUNDS];

      return {
        ...base,
        center,
        min_zoom: data.min_zoom ?? 0,
        max_zoom: data.max_zoom ?? 22,
        fillzoom: data.fillzoom ?? 12,
        bounds,
        layers: layerRefs,
      };
    });

    return {
      version: '0.1.0',
      nodes: serializedNodes,
      edges: edges.map((edge) => ({ id: edge.id, source: edge.source, target: edge.target })),
    };
  }, [nodes, edges]);

  const handleVerify = async () => {
    setIsValidating(true);
    setValidationErrors([]);
    setShowSuccess(false);
    setStatusMessage('');

    try {
      const result = await api.verifyConfig(buildConfig());
      if (result.valid) {
        setShowSuccess(true);
        setTimeout(() => setShowSuccess(false), 3000);
      } else {
        setValidationErrors(result.errors || []);
      }
    } catch (error) {
      console.error('Verification failed:', error);
      setStatusMessage(`Verification failed: ${error.message}`);
    } finally {
      setIsValidating(false);
    }
  };

  const handleApply = async () => {
    if (validationErrors.length > 0) {
      setStatusMessage('Please fix validation errors before applying.');
      return;
    }

    setIsApplying(true);
    setStatusMessage('');

    try {
      await api.applyConfig(buildConfig());
      setShowSuccess(true);
      setTimeout(() => setShowSuccess(false), 3000);
    } catch (error) {
      console.error('Apply failed:', error);
      setStatusMessage(`Apply failed: ${error.message}`);
    } finally {
      setIsApplying(false);
    }
  };

  const handleUpload = async (event) => {
    const file = event.target.files[0];
    if (!file) return;

    try {
      const result = await api.uploadFile(file, uploadSrid || null);
      const position = {
        x: TYPE_COLUMNS.resource,
        y: 120 + resourceNodes.length * 180,
      };
      setNodes((nds) => [
        ...nds,
        {
          id: result.node.id,
          type: 'resource',
          position,
          data: result.node,
        },
      ]);
      setUploadSrid('');
      setStatusMessage('Upload successful. Resource node created.');
    } catch (error) {
      console.error('Upload failed:', error);
      setStatusMessage(`Upload failed: ${error.message}`);
    }
  };

  const requestMetadata = async (resourceId) => {
    if (!resourceId) return;
    try {
      const meta = await api.getResourceMetadata(resourceId);
      setResourceMeta((prev) => ({ ...prev, [resourceId]: meta }));
      setStatusMessage('Resource metadata loaded.');
      return meta;
    } catch (error) {
      console.error('Metadata fetch failed:', error);
      setStatusMessage(`Failed to load metadata: ${error.message}`);
    }
  };

  const applyXyzBounds = async (xyzId) => {
    const xyzNode = nodeById.get(xyzId);
    if (!xyzNode?.data.layers?.length) {
      setStatusMessage('Connect at least one layer to derive bounds.');
      return;
    }

    const firstLayerId = xyzNode.data.layers[0].source_layer_id;
    const layerNode = nodeById.get(firstLayerId);
    const resourceId = layerNode?.data.source_resource_id;
    if (!resourceId) {
      setStatusMessage('Layer must be connected to a resource first.');
      return;
    }

    let meta = resourceMeta[resourceId];
    if (!meta) {
      meta = await requestMetadata(resourceId);
    }

    const bounds = meta?.bounds;
    if (bounds) {
      updateNodeData(xyzId, { bounds });
    }
  };

  const applyXyzCenter = async (xyzId) => {
    const xyzNode = nodeById.get(xyzId);
    if (!xyzNode?.data.layers?.length) {
      setStatusMessage('Connect at least one layer to derive center.');
      return;
    }

    const firstLayerId = xyzNode.data.layers[0].source_layer_id;
    const layerNode = nodeById.get(firstLayerId);
    const resourceId = layerNode?.data.source_resource_id;
    if (!resourceId) {
      setStatusMessage('Layer must be connected to a resource first.');
      return;
    }

    let meta = resourceMeta[resourceId];
    if (!meta) {
      meta = await requestMetadata(resourceId);
    }

    const center = meta?.center;
    if (center) {
      updateNodeData(xyzId, { center: [center[0], center[1], xyzNode.data.center?.[2] ?? 6] });
    }
  };

  return (
    <div className="app">
      <div className="control-panel">
        <div className="brand">
          <div className="brand__title">MapFlow Studio</div>
          <div className="brand__subtitle">Drag, connect, and publish tiles in minutes.</div>
        </div>

        <div className="panel-section">
          <label className="panel-label">Upload Shapefile (ZIP)</label>
          <input className="input" type="file" accept=".zip" onChange={handleUpload} />
          <label className="panel-label">SRID Override (optional)</label>
          <input
            className="input"
            value={uploadSrid}
            onChange={(event) => setUploadSrid(event.target.value)}
            placeholder="4326"
          />
        </div>

        <div className="panel-section panel-actions">
          <button
            className="button"
            onClick={handleVerify}
            disabled={isValidating || nodes.length === 0}
          >
            {isValidating ? 'Verifying...' : 'Validate'}
          </button>
          <button
            className="button button--accent"
            onClick={handleApply}
            disabled={isApplying || validationErrors.length > 0 || nodes.length === 0}
          >
            {isApplying ? 'Applying...' : 'Apply'}
          </button>
        </div>

        {showSuccess && <div className="panel-alert panel-alert--success">Success!</div>}
        {statusMessage && <div className="panel-alert">{statusMessage}</div>}

        {validationErrors.length > 0 && (
          <div className="panel-alert panel-alert--error">
            <div className="panel-alert__title">Validation errors</div>
            {validationErrors.map((error, index) => (
              <div key={index} className="panel-alert__item">
                {error.node_id ? `Node ${error.node_id}: ` : ''}
                {error.message}
              </div>
            ))}
          </div>
        )}

        <div className="panel-section panel-help">
          <div>Right click the canvas to add Layer or XYZ nodes.</div>
          <div>Double-click a node to edit its properties.</div>
          <div>Connect nodes in sequence: Resource → Layer → XYZ.</div>
        </div>
      </div>

      {inspectorOpen && (
        <NodeInspector
          node={selectedNode}
          resources={resourceNodes}
          layers={layerNodes}
          resourceMeta={resourceMeta}
          onClose={() => setInspectorOpen(false)}
          onUpdateNode={updateNodeData}
          onLayerSourceChange={syncLayerSourceEdge}
          onRequestMetadata={requestMetadata}
          onToggleXyzLayer={toggleXyzLayer}
          onApplyXyzBounds={applyXyzBounds}
          onApplyXyzCenter={applyXyzCenter}
        />
      )}

      {contextMenu && (
        <div className="context-menu" style={{ top: contextMenu.y, left: contextMenu.x }}>
          <button className="context-menu__item" onClick={() => handleContextCreate('layer')}>
            Add Layer Node
          </button>
          <button className="context-menu__item" onClick={() => handleContextCreate('xyz')}>
            Add XYZ Node
          </button>
        </div>
      )}

      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onNodesDelete={onNodesDelete}
        onEdgesChange={onEdgesChange}
        onEdgesDelete={onEdgesDelete}
        onConnect={onConnect}
        isValidConnection={isValidConnection}
        nodeTypes={nodeTypes}
        onNodeClick={(_, node) => setSelectedNodeId(node.id)}
        onNodeDoubleClick={onNodeDoubleClick}
        onPaneContextMenu={onPaneContextMenu}
        onPaneClick={onPaneClick}
        deleteKeyCode={['Backspace', 'Delete']}
        fitView
      >
        <Background gap={20} size={1} color="rgba(148, 163, 184, 0.35)" />
        <Controls />
        {nodes.length > 0 && nodesInitialized && (
          <MiniMap
            nodeColor={(node) =>
              node.type === 'resource'
                ? '#10b981'
                : node.type === 'layer'
                  ? '#3b82f6'
                  : '#f59e0b'
            }
          />
        )}
      </ReactFlow>
    </div>
  );
}

function App() {
  return (
    <ReactFlowProvider>
      <AppShell />
    </ReactFlowProvider>
  );
}

export default App;
