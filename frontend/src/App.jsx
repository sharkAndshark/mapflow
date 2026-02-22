import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useAuth } from './AuthContext.jsx';
import {
  hasActiveJobs as computeHasActiveJobs,
  mergeServerFilesWithOptimistic,
} from './polling.js';
import { publishFile, unpublishFile, updateTileZoom, updateFieldAliases } from './api.js';
import { formatSize, parseType, validateSlug } from './utils.js';

const STATUS_LABELS = {
  uploading: '上传中',
  uploaded: '等待处理',
  processing: '处理中',
  ready: '已就绪',
  failed: '失败',
};

function DetailSidebar({ file, onZoomUpdate, onPublish, onUnpublish }) {
  // Tab state
  const [activeTab, setActiveTab] = useState('basic');
  const tabs = [
    { id: 'basic', label: 'Basic Info' },
    { id: 'fields', label: 'Fields' },
    { id: 'publish', label: 'Publish' },
  ];

  const [schema, setSchema] = useState(null);
  const [schemaError, setSchemaError] = useState(null);
  const [isLoadingSchema, setIsLoadingSchema] = useState(false);
  const [editZoom, setEditZoom] = useState(false);
  const [minZoom, setMinZoom] = useState(0);
  const [maxZoom, setMaxZoom] = useState(22);
  const [zoomError, setZoomError] = useState('');
  const [isSavingZoom, setIsSavingZoom] = useState(false);
  const [editingAlias, setEditingAlias] = useState(null);
  const [aliasValue, setAliasValue] = useState('');
  const [aliasError, setAliasError] = useState('');
  const [isSavingAlias, setIsSavingAlias] = useState(false);
  const aliasEditRef = useRef(null);

  // Handle click outside to cancel alias editing
  useEffect(() => {
    if (!editingAlias) return;

    const handleClickOutside = (event) => {
      if (aliasEditRef.current && !aliasEditRef.current.contains(event.target)) {
        setEditingAlias(null);
        setAliasValue('');
        setAliasError('');
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, [editingAlias]);

  // Publish-related state
  const [editPublish, setEditPublish] = useState(false);
  const [publishSlug, setPublishSlug] = useState('');
  const [publishMinZoom, setPublishMinZoom] = useState(0);
  const [publishMaxZoom, setPublishMaxZoom] = useState(22);
  const [publishError, setPublishError] = useState('');
  const [isPublishing, setIsPublishing] = useState(false);
  const [copySuccess, setCopySuccess] = useState(false);

  // Cache slug validation result to avoid duplicate calls
  const slugValidationError = useMemo(() => {
    return validateSlug(publishSlug.trim()).error;
  }, [publishSlug]);

  // Real-time zoom validation for non-tile files
  const isTileFile = file?.tileFormat != null;
  const zoomValidationError = useMemo(() => {
    if (isTileFile) return null;
    return publishMinZoom > publishMaxZoom ? '最小层级不能大于最大层级' : null;
  }, [publishMinZoom, publishMaxZoom, isTileFile]);

  useEffect(() => {
    const fileId = file?.id;
    const fileStatus = file?.status;

    if (!fileId || fileStatus !== 'ready') {
      setSchema(null);
      setSchemaError(null);
      return;
    }

    let cancelled = false;
    setIsLoadingSchema(true);
    setSchemaError(null);

    fetch(`/api/files/${fileId}/schema`)
      .then(async (res) => {
        if (!res.ok) {
          const data = await res.json().catch(() => ({}));
          throw new Error(data.error || 'Failed to load schema');
        }
        return res.json();
      })
      .then((data) => {
        if (!cancelled) {
          setSchema(data);
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setSchemaError(err.message);
        }
      })
      .finally(() => {
        if (!cancelled) {
          setIsLoadingSchema(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [file?.id, file?.status]);

  useEffect(() => {
    if (file) {
      setMinZoom(file.minZoom ?? 0);
      setMaxZoom(file.maxZoom ?? 22);
      setEditZoom(false);
      setZoomError('');
      setEditingAlias(null);
      setAliasValue('');
      setAliasError('');
      // Reset publish-related state when file changes
      setEditPublish(false);
      setPublishSlug('');
      setPublishMinZoom(file.minZoom ?? 0);
      setPublishMaxZoom(file.maxZoom ?? 22);
      setPublishError('');
      setCopySuccess(false);
    }
  }, [file]);

  // Reset to basic tab only when file ID changes (different file selected)
  // Using file?.id instead of [file] prevents tab reset when file properties change
  // (e.g., when isPublic changes after publishing)
  useEffect(() => {
    setActiveTab('basic');
  }, [file?.id]);

  function copyPublicUrl(slug) {
    if (!slug) {
      return;
    }
    const url = `${window.location.origin}/tiles/${slug}/{z}/{x}/{y}`;
    navigator.clipboard
      .writeText(url)
      .then(() => {
        setCopySuccess(true);
        setTimeout(() => setCopySuccess(false), 2000);
      })
      .catch(() => {
        alert('复制失败，请手动复制地址');
      });
  }

  async function handlePublishSubmit() {
    if (!file) return;

    setPublishError('');
    setIsPublishing(true);

    try {
      const options = {
        slug: publishSlug.trim() || undefined,
      };
      if (!isTileFile) {
        options.minZoom = publishMinZoom;
        options.maxZoom = publishMaxZoom;
      }
      await onPublish(file.id, options);
      setEditPublish(false);
    } catch (err) {
      setPublishError(err.message || '发布失败');
    } finally {
      setIsPublishing(false);
    }
  }

  async function handleUnpublishClick() {
    if (!file) return;
    if (!confirm(`确定取消发布 "${file.name}" 吗？`)) return;
    try {
      await onUnpublish(file.id);
    } catch (err) {
      setPublishError(err.message || '取消发布失败');
    }
  }

  if (!file) {
    return (
      <div className="detail-empty">
        <p>选择一个文件查看详情</p>
      </div>
    );
  }

  const isReady = file.status === 'ready';
  const isFailed = file.status === 'failed';

  return (
    <div className="detail-sidebar" data-testid="detail-sidebar">
      <div className="detail-content">
        <div className="detail-header">
          <h3 className="detail-title">{file.name}</h3>
          <span className="detail-id">{file.id}</span>
        </div>

        {/* Tab navigation */}
        <div className="tab-nav" role="tablist">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              type="button"
              className={`tab-btn ${activeTab === tab.id ? 'active' : ''}`}
              onClick={() => setActiveTab(tab.id)}
              role="tab"
              aria-selected={activeTab === tab.id}
              aria-controls={`tabpanel-${tab.id}`}
            >
              {tab.label}
            </button>
          ))}
        </div>

        {/* Basic Info Tab */}
        {activeTab === 'basic' && (
          <div className="tab-content" role="tabpanel" id="tabpanel-basic">
            <div className="detail-group">
              <div className="detail-label">Type</div>
              <div className="detail-value">{file.type}</div>
            </div>

            <div className="detail-group">
              <div className="detail-label">Size</div>
              <div className="detail-value">{formatSize(file.size || 0)}</div>
            </div>

            <div className="detail-group">
              <div className="detail-label">Status</div>
              <div className={`status ${file.status}`} data-testid="file-status">
                {STATUS_LABELS[file.status] || file.status}
              </div>
            </div>

            <div className="detail-group">
              <div className="detail-label">Uploaded At</div>
              <div className="detail-value">
                {file.uploadedAt ? new Date(file.uploadedAt).toLocaleString() : '--'}
              </div>
            </div>

            {file.crs && (
              <div className="detail-group">
                <div className="detail-label">CRS</div>
                <div className="detail-value">{file.crs}</div>
              </div>
            )}

            {isFailed && file.error && (
              <div className="detail-error">
                <strong>Error:</strong> {file.error}
              </div>
            )}
          </div>
        )}

        {/* Fields Tab */}
        {activeTab === 'fields' &&
          (isReady ? (
            <div className="tab-content fields-section" role="tabpanel" id="tabpanel-fields">
              {isLoadingSchema ? (
                <span style={{ color: '#888', fontSize: '12px' }}>加载中...</span>
              ) : schemaError ? (
                <span style={{ color: '#d32f2f', fontSize: '12px' }}>{schemaError}</span>
              ) : schema?.layers ? (
                <div style={{ fontSize: '13px' }}>
                  {schema.layers.length === 0 ? (
                    <span style={{ color: '#888' }}>无字段</span>
                  ) : (
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
                      {schema.layers.map((layer) => (
                        <div key={layer.id}>
                          <div
                            style={{
                              fontWeight: 600,
                              fontSize: '12px',
                              color: '#444',
                              marginBottom: '4px',
                              paddingBottom: '4px',
                              borderBottom: '1px solid #e0e0e0',
                            }}
                          >
                            {layer.description ? `${layer.id} - ${layer.description}` : layer.id}
                          </div>
                          <table
                            className="fields-table"
                            style={{ width: '100%', fontSize: '12px', borderCollapse: 'collapse' }}
                          >
                            <thead>
                              <tr style={{ borderBottom: '1px solid #e0e0e0' }}>
                                <th
                                  style={{
                                    textAlign: 'left',
                                    padding: '4px 8px',
                                    color: '#666',
                                    fontWeight: 500,
                                  }}
                                >
                                  原始名称
                                </th>
                                <th
                                  style={{
                                    textAlign: 'left',
                                    padding: '4px 8px',
                                    color: '#666',
                                    fontWeight: 500,
                                  }}
                                >
                                  别名
                                </th>
                                <th
                                  style={{
                                    textAlign: 'left',
                                    padding: '4px 8px',
                                    color: '#666',
                                    fontWeight: 500,
                                  }}
                                >
                                  类型
                                </th>
                              </tr>
                            </thead>
                            <tbody>
                              {layer.fields.length === 0 ? (
                                <tr>
                                  <td
                                    colSpan="3"
                                    style={{ padding: '8px', color: '#999', fontStyle: 'italic' }}
                                  >
                                    无字段
                                  </td>
                                </tr>
                              ) : (
                                layer.fields.map((field) => {
                                  const fieldKey = field.normalized || field.name;
                                  const isEditing = editingAlias === fieldKey;

                                  const handleStartEdit = () => {
                                    setEditingAlias(fieldKey);
                                    setAliasValue(field.alias || '');
                                    setAliasError('');
                                  };

                                  const handleCancel = () => {
                                    setEditingAlias(null);
                                    setAliasValue('');
                                    setAliasError('');
                                  };

                                  const handleSave = async () => {
                                    const trimmedValue = aliasValue.trim();
                                    if (trimmedValue.length > 255) {
                                      setAliasError('别名不能超过 255 个字符');
                                      return;
                                    }
                                    setAliasError('');
                                    setIsSavingAlias(true);
                                    try {
                                      await updateFieldAliases(file.id, [
                                        {
                                          normalized_name: fieldKey,
                                          alias: trimmedValue || null,
                                        },
                                      ]);
                                      setSchema((prev) => {
                                        if (!prev) return prev;
                                        return {
                                          ...prev,
                                          layers: prev.layers.map((l) => ({
                                            ...l,
                                            fields: l.fields.map((f) =>
                                              (f.normalized || f.name) === fieldKey
                                                ? { ...f, alias: trimmedValue || null }
                                                : f,
                                            ),
                                          })),
                                        };
                                      });
                                      setEditingAlias(null);
                                      setAliasValue('');
                                    } catch (err) {
                                      setAliasError(err.message || '保存失败');
                                    } finally {
                                      setIsSavingAlias(false);
                                    }
                                  };

                                  const handleKeyDown = (e) => {
                                    if (e.key === 'Enter') {
                                      e.preventDefault();
                                      handleSave();
                                    } else if (e.key === 'Escape') {
                                      e.preventDefault();
                                      handleCancel();
                                    }
                                  };

                                  return (
                                    <tr
                                      key={fieldKey}
                                      style={{ borderBottom: '1px solid #f0f0f0' }}
                                    >
                                      <td style={{ padding: '6px 8px' }}>
                                        <span style={{ fontWeight: 500 }}>{field.name}</span>
                                      </td>
                                      <td
                                        ref={isEditing ? aliasEditRef : null}
                                        className={`alias-cell ${isEditing ? 'editing' : ''}`}
                                        style={{ padding: '6px 8px', verticalAlign: 'top' }}
                                        onClick={!isEditing ? handleStartEdit : undefined}
                                      >
                                        {isEditing ? (
                                          <div>
                                            <input
                                              type="text"
                                              value={aliasValue}
                                              onChange={(e) => setAliasValue(e.target.value)}
                                              onKeyDown={handleKeyDown}
                                              placeholder="输入别名"
                                              className="alias-input"
                                              disabled={isSavingAlias}
                                              autoFocus
                                            />
                                            {aliasError && (
                                              <div
                                                style={{
                                                  fontSize: '11px',
                                                  color: '#d32f2f',
                                                  marginTop: '4px',
                                                }}
                                              >
                                                {aliasError}
                                              </div>
                                            )}
                                            <div className="alias-buttons">
                                              <button
                                                type="button"
                                                className="btn-primary"
                                                disabled={isSavingAlias}
                                                onClick={handleSave}
                                              >
                                                {isSavingAlias ? '...' : '保存'}
                                              </button>
                                              <button
                                                type="button"
                                                className="btn-secondary"
                                                onClick={handleCancel}
                                              >
                                                取消
                                              </button>
                                            </div>
                                          </div>
                                        ) : (
                                          <span style={{ color: field.alias ? '#333' : '#999' }}>
                                            {field.alias || '-'}
                                          </span>
                                        )}
                                      </td>
                                      <td style={{ padding: '6px 8px' }}>
                                        <span
                                          style={{
                                            fontSize: '11px',
                                            color: '#666',
                                            background: '#f5f5f5',
                                            padding: '1px 6px',
                                            borderRadius: '3px',
                                          }}
                                        >
                                          {field.type}
                                        </span>
                                      </td>
                                    </tr>
                                  );
                                })
                              )}
                            </tbody>
                          </table>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              ) : null}
            </div>
          ) : (
            <div className="tab-empty" role="tabpanel" id="tabpanel-fields">
              文件处理完成后可查看字段信息
            </div>
          ))}

        {/* Publish Tab */}
        {activeTab === 'publish' &&
          (isReady ? (
            <div className="tab-content" role="tabpanel" id="tabpanel-publish">
              {/* Publish status section */}
              {!file.isPublic && !editPublish && (
                <div className="detail-group">
                  <div className="detail-label">发布状态</div>
                  <div className="detail-value">
                    <div
                      style={{
                        display: 'flex',
                        justifyContent: 'space-between',
                        alignItems: 'center',
                      }}
                    >
                      <span style={{ color: '#888' }}>未发布</span>
                      <button
                        type="button"
                        className="btn-primary"
                        style={{ fontSize: '12px', padding: '4px 12px' }}
                        onClick={() => setEditPublish(true)}
                      >
                        发布
                      </button>
                    </div>
                  </div>
                </div>
              )}

              {!file.isPublic && editPublish && (
                <div className="detail-group">
                  <div className="detail-label">发布设置</div>
                  <div className="detail-value">
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
                      {/* URL Slug */}
                      <div>
                        <label
                          style={{
                            fontSize: '12px',
                            color: '#666',
                            marginBottom: '4px',
                            display: 'block',
                          }}
                        >
                          URL 标识（可选）
                        </label>
                        <input
                          type="text"
                          value={publishSlug}
                          onChange={(e) => setPublishSlug(e.target.value)}
                          placeholder={file.id}
                          className="form-input"
                          style={{ width: '100%' }}
                          data-testid="publish-slug-input"
                        />
                        {slugValidationError && (
                          <div className="alert" style={{ marginTop: '4px', fontSize: '12px' }}>
                            {slugValidationError}
                          </div>
                        )}
                        <small className="form-hint">
                          留空则使用文件 ID。仅支持字母、数字、连字符和下划线
                        </small>
                      </div>

                      {/* Zoom levels */}
                      <div>
                        <label
                          style={{
                            fontSize: '12px',
                            color: '#666',
                            marginBottom: '4px',
                            display: 'block',
                          }}
                        >
                          缩放层级
                          {file.tileFormat != null && (
                            <span style={{ color: '#888', fontWeight: 'normal' }}> (只读)</span>
                          )}
                        </label>
                        <div style={{ display: 'flex', gap: '12px', alignItems: 'center' }}>
                          <div style={{ flex: 1 }}>
                            <small className="form-hint">最小</small>
                            {file.tileFormat != null ? (
                              <div className="form-value">{file.minZoom ?? '-'}</div>
                            ) : (
                              <input
                                type="number"
                                min="0"
                                max="22"
                                value={publishMinZoom}
                                onChange={(e) => setPublishMinZoom(parseInt(e.target.value) || 0)}
                                className="form-input"
                                style={{ width: '100%' }}
                              />
                            )}
                          </div>
                          <div style={{ flex: 1 }}>
                            <small className="form-hint">最大</small>
                            {file.tileFormat != null ? (
                              <div className="form-value">{file.maxZoom ?? '-'}</div>
                            ) : (
                              <input
                                type="number"
                                min="0"
                                max="22"
                                value={publishMaxZoom}
                                onChange={(e) => setPublishMaxZoom(parseInt(e.target.value) || 22)}
                                className="form-input"
                                style={{ width: '100%' }}
                              />
                            )}
                          </div>
                        </div>
                        {file.tileFormat == null && (
                          <small className="form-hint">动态矢量数据可设置 0-22 层级范围</small>
                        )}
                        {zoomValidationError && (
                          <div className="alert" style={{ marginTop: '4px', fontSize: '12px' }}>
                            {zoomValidationError}
                          </div>
                        )}
                      </div>

                      {/* Public URL preview */}
                      <div>
                        <label
                          style={{
                            fontSize: '12px',
                            color: '#666',
                            marginBottom: '4px',
                            display: 'block',
                          }}
                        >
                          公开地址
                        </label>
                        <div className="form-value code" style={{ fontSize: '12px' }}>
                          /tiles/{publishSlug.trim() || file.id}/{'{z}/{x}/{y}'}
                        </div>
                      </div>

                      {publishError && (
                        <div className="alert" style={{ margin: 0 }}>
                          {publishError}
                        </div>
                      )}

                      {/* Action buttons */}
                      <div style={{ display: 'flex', gap: '8px' }}>
                        <button
                          type="button"
                          className="btn-primary"
                          style={{ fontSize: '12px', padding: '4px 12px' }}
                          disabled={isPublishing || !!slugValidationError || !!zoomValidationError}
                          onClick={handlePublishSubmit}
                        >
                          {isPublishing ? '发布中...' : '确认发布'}
                        </button>
                        <button
                          type="button"
                          className="btn-secondary"
                          style={{ fontSize: '12px', padding: '4px 12px' }}
                          onClick={() => {
                            setEditPublish(false);
                            setPublishSlug('');
                            setPublishMinZoom(file.minZoom ?? 0);
                            setPublishMaxZoom(file.maxZoom ?? 22);
                            setPublishError('');
                          }}
                        >
                          取消
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {file.isPublic && (
                <>
                  <div className="detail-group">
                    <div className="detail-label">发布状态</div>
                    <div className="detail-value">
                      <span style={{ color: '#4caf50' }}>已发布</span>
                    </div>
                  </div>

                  <div className="detail-group">
                    <div className="detail-label">公开地址</div>
                    <div className="detail-value">
                      <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
                        <div className="form-value code" style={{ fontSize: '12px' }}>
                          /tiles/{file.publicSlug}/{'{z}/{x}/{y}'}
                        </div>
                        <button
                          type="button"
                          className="btn-text"
                          style={{ fontSize: '12px', padding: 0, textAlign: 'left' }}
                          onClick={() => copyPublicUrl(file.publicSlug)}
                        >
                          {copySuccess ? '已复制' : '复制地址'}
                        </button>
                      </div>
                    </div>
                  </div>
                </>
              )}

              {file.isPublic && (
                <div className="detail-group">
                  <div className="detail-label">缩放层级</div>
                  <div className="detail-value">
                    {editZoom ? (
                      <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
                        <div style={{ display: 'flex', gap: '12px', alignItems: 'center' }}>
                          <div style={{ flex: 1 }}>
                            <small className="form-hint">最小</small>
                            <input
                              type="number"
                              min="0"
                              max="22"
                              value={minZoom}
                              onChange={(e) => setMinZoom(parseInt(e.target.value) || 0)}
                              className="form-input"
                              style={{ width: '100%' }}
                            />
                          </div>
                          <div style={{ flex: 1 }}>
                            <small className="form-hint">最大</small>
                            <input
                              type="number"
                              min="0"
                              max="22"
                              value={maxZoom}
                              onChange={(e) => setMaxZoom(parseInt(e.target.value) || 22)}
                              className="form-input"
                              style={{ width: '100%' }}
                            />
                          </div>
                        </div>
                        {zoomError && (
                          <div className="alert" style={{ margin: 0 }}>
                            {zoomError}
                          </div>
                        )}
                        <div style={{ display: 'flex', gap: '8px' }}>
                          <button
                            type="button"
                            className="btn-primary"
                            style={{ fontSize: '12px', padding: '4px 12px' }}
                            disabled={isSavingZoom}
                            onClick={async () => {
                              if (minZoom > maxZoom) {
                                setZoomError('最小层级不能大于最大层级');
                                return;
                              }
                              setZoomError('');
                              setIsSavingZoom(true);
                              try {
                                await updateTileZoom(file.id, minZoom, maxZoom);
                                setEditZoom(false);
                                if (onZoomUpdate) {
                                  onZoomUpdate(file.id, minZoom, maxZoom);
                                }
                              } catch (err) {
                                setZoomError(err.message || '保存失败');
                              } finally {
                                setIsSavingZoom(false);
                              }
                            }}
                          >
                            {isSavingZoom ? '保存中...' : '保存'}
                          </button>
                          <button
                            type="button"
                            className="btn-secondary"
                            style={{ fontSize: '12px', padding: '4px 12px' }}
                            onClick={() => {
                              setMinZoom(file.minZoom ?? 0);
                              setMaxZoom(file.maxZoom ?? 22);
                              setEditZoom(false);
                              setZoomError('');
                            }}
                          >
                            取消
                          </button>
                        </div>
                      </div>
                    ) : (
                      <div
                        style={{
                          display: 'flex',
                          justifyContent: 'space-between',
                          alignItems: 'center',
                        }}
                      >
                        <span>
                          {file.minZoom ?? 0} ~ {file.maxZoom ?? 22}
                        </span>
                        {file.tileFormat == null && (
                          <button
                            type="button"
                            className="btn-text"
                            style={{ fontSize: '12px' }}
                            onClick={() => setEditZoom(true)}
                          >
                            修改
                          </button>
                        )}
                      </div>
                    )}
                    {file.tileFormat != null && !editZoom && (
                      <small className="form-hint" style={{ marginTop: '4px' }}>
                        瓦片文件的缩放层级由源文件决定
                      </small>
                    )}
                  </div>
                </div>
              )}

              {/* Unpublish button */}
              {file.isPublic && (
                <div className="detail-group">
                  <button
                    type="button"
                    className="btn-secondary"
                    style={{ width: '100%', fontSize: '12px' }}
                    onClick={handleUnpublishClick}
                  >
                    取消发布
                  </button>
                </div>
              )}
            </div>
          ) : (
            <div className="tab-empty" role="tabpanel" id="tabpanel-publish">
              文件处理完成后可进行发布操作
            </div>
          ))}
      </div>
    </div>
  );
}

export default function App() {
  const { user, logout } = useAuth();
  const [files, setFiles] = useState([]);
  const [selectedId, setSelectedId] = useState(null);
  const [errorMessage, setErrorMessage] = useState('');
  const [isLoading, setIsLoading] = useState(true);

  async function handleLogout() {
    try {
      await logout();
      window.location.href = '/login';
    } catch (error) {
      console.error('Logout failed:', error);
    }
  }

  async function handlePublish(fileId, options) {
    const result = await publishFile(fileId, options);
    setFiles((prev) =>
      prev.map((f) => (f.id === fileId ? { ...f, isPublic: true, publicSlug: result.slug } : f)),
    );
  }

  async function handleUnpublish(fileId) {
    await unpublishFile(fileId);
    setFiles((prev) =>
      prev.map((f) => (f.id === fileId ? { ...f, isPublic: false, publicSlug: null } : f)),
    );
  }

  function handleZoomUpdate(fileId, minZoom, maxZoom) {
    setFiles((prev) => prev.map((f) => (f.id === fileId ? { ...f, minZoom, maxZoom } : f)));
  }

  // Derive selected file object
  const selectedFile = useMemo(
    () => files.find((f) => f.id === selectedId) || null,
    [files, selectedId],
  );

  const hasActiveJobs = useMemo(() => computeHasActiveJobs(files), [files]);

  // Polling Logic
  useEffect(() => {
    if (!hasActiveJobs) return;

    const intervalId = setInterval(async () => {
      try {
        const res = await fetch('/api/files');
        if (!res.ok) return;
        const data = await res.json();

        setFiles((prevFiles) => {
          return mergeServerFilesWithOptimistic(prevFiles, data);
        });
      } catch (err) {
        console.error('Polling failed', err);
      }
    }, 2000); // Poll every 2 seconds

    return () => clearInterval(intervalId);
  }, [hasActiveJobs]);

  useEffect(() => {
    let cancelled = false;
    async function fetchFiles() {
      try {
        const res = await fetch('/api/files');
        const data = await res.json();
        if (!cancelled) {
          setFiles(Array.isArray(data) ? data : []);
        }
      } catch (error) {
        if (!cancelled) {
          setErrorMessage('无法加载文件列表');
        }
      } finally {
        if (!cancelled) {
          setIsLoading(false);
        }
      }
    }
    fetchFiles();
    return () => {
      cancelled = true;
    };
  }, []);

  const orderedFiles = useMemo(() => {
    return [...files].sort((a, b) => {
      if (!a.uploadedAt || !b.uploadedAt) return 0;
      return b.uploadedAt.localeCompare(a.uploadedAt);
    });
  }, [files]);

  async function handleFileChange(event) {
    const file = event.target.files?.[0];
    event.target.value = '';
    if (!file) return;

    setErrorMessage('');

    const tempId = `temp-${Date.now()}`;
    const optimistic = {
      id: tempId,
      name: file.name.replace(/\.[^/.]+$/, ''),
      type: parseType(file.name),
      size: file.size,
      uploadedAt: new Date().toISOString(),
      status: 'uploading',
      crs: null,
    };

    setFiles((prev) => [optimistic, ...prev]);
    // Auto-select the uploading file
    setSelectedId(tempId);

    const formData = new FormData();
    formData.append('file', file);

    try {
      const res = await fetch('/api/uploads', {
        method: 'POST',
        body: formData,
      });
      if (!res.ok) {
        const data = await res.json().catch(() => ({}));
        throw new Error(data.error || '上传失败');
      }
      const data = await res.json();
      setFiles((prev) => [data, ...prev.filter((item) => item.id !== tempId)]);
      setSelectedId(data.id);
    } catch (error) {
      const message = error instanceof Error ? error.message : '上传失败';
      setErrorMessage(message);
      setFiles((prev) =>
        prev.map((item) =>
          item.id === tempId ? { ...item, status: 'failed', error: message } : item,
        ),
      );
    }
  }

  return (
    <div className="page">
      <header className="header">
        <div>
          <h1>MapFlow</h1>
          <p className="subtitle">文件上传与列表</p>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: '1rem' }}>
          {user && (
            <span style={{ fontSize: '14px', color: '#666' }}>
              {user.username} ({user.role})
            </span>
          )}
          <label className="upload-button">
            <input
              type="file"
              accept=".zip,.geojson,.json,.geojsonl,.geojsons,.kml,.gpx,.topojson,.mbtiles"
              onChange={handleFileChange}
              data-testid="file-input"
            />
            上传
          </label>
          {user && (
            <button type="button" className="btn-secondary" onClick={handleLogout}>
              登出
            </button>
          )}
        </div>
      </header>

      {errorMessage ? <div className="alert">{errorMessage}</div> : null}

      <section className="panel">
        <div className="panel-header">
          <h2>上传文件</h2>
          <span className="panel-meta">
            支持 .zip / .geojson / .geojsonl / .kml / .gpx / .topojson / .mbtiles，单文件最大
            200MB（可配置）
          </span>
        </div>

        <div className="panel-body">
          <div className="list-area">
            {isLoading ? (
              <div className="empty">加载中...</div>
            ) : orderedFiles.length === 0 ? (
              <div className="empty" data-testid="empty-state">
                暂未上传文件
              </div>
            ) : (
              <div className="table">
                <div className="row head">
                  <div>名称</div>
                  <div>类型</div>
                  <div>大小</div>
                  <div>上传时间</div>
                  <div>状态</div>
                  <div></div>
                </div>
                {orderedFiles.map((item) => (
                  <div
                    key={item.id}
                    className={`row ${selectedId === item.id ? 'selected' : ''}`}
                    onClick={() => setSelectedId(item.id)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        setSelectedId(item.id);
                      }
                    }}
                    tabIndex={0}
                    role="button"
                    aria-pressed={selectedId === item.id}
                    data-testid={`file-row-${item.id}`}
                  >
                    <div>{item.name}</div>
                    <div>{item.type}</div>
                    <div>{formatSize(item.size || 0)}</div>
                    <div className="muted">
                      {item.uploadedAt ? new Date(item.uploadedAt).toLocaleString() : '--'}
                    </div>
                    <div className={`status ${item.status || 'uploaded'}`}>
                      {STATUS_LABELS[item.status] || item.status}
                    </div>
                    <div onClick={(e) => e.stopPropagation()}>
                      {item.status === 'ready' && (
                        <a
                          href={`/preview/${item.id}`}
                          target="_blank"
                          rel="noopener noreferrer"
                          className="btn-text"
                          title="在新窗口查看地图"
                        >
                          查看
                        </a>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="detail-area">
            <DetailSidebar
              file={selectedFile}
              onZoomUpdate={handleZoomUpdate}
              onPublish={handlePublish}
              onUnpublish={handleUnpublish}
            />
          </div>
        </div>
      </section>
    </div>
  );
}
