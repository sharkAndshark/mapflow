import React, { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useAuth } from './AuthContext.jsx';
import {
  hasActiveJobs as computeHasActiveJobs,
  mergeServerFilesWithOptimistic,
} from './polling.js';
import {
  publishFile,
  registerPostgisSource,
  testPostgisConnection,
  unpublishFile,
  updateTileZoom,
  updateFieldAliases,
  updatePublishSettings,
} from './api.js';
import { formatSize, parseType, validateSlug } from './utils.js';
import LanguageSwitcher from './LanguageSwitcher.jsx';

const INITIAL_POSTGIS_FORM = {
  connectionName: '',
  host: '127.0.0.1',
  port: 5432,
  database: '',
  username: '',
  password: '',
  sslMode: 'disable',
  schema: 'public',
  object: '',
  geometryColumn: 'geom',
  fidColumn: 'id',
  displayName: '',
};

const STATUS_LABEL_KEYS = {
  uploading: 'file.status.uploading',
  uploaded: 'file.status.uploaded',
  processing: 'file.status.processing',
  ready: 'file.status.ready',
  failed: 'file.status.failed',
};

function getStatusLabel(t, status) {
  const key = STATUS_LABEL_KEYS[status];
  return key ? t(key) : status;
}

function DetailSidebar({ file, onZoomUpdate, onPublish, onUnpublish, onUseAliasesUpdate }) {
  const { t } = useTranslation();

  const [activeTab, setActiveTab] = useState('basic');
  const tabs = [
    { id: 'basic', label: t('file.detail.basicInfo') },
    { id: 'fields', label: t('file.detail.fields') },
    { id: 'publish', label: t('file.detail.publish') },
  ];

  const [schema, setSchema] = useState(null);
  const [schemaError, setSchemaError] = useState(null);
  const [isLoadingSchema, setIsLoadingSchema] = useState(false);
  const [editZoom, setEditZoom] = useState(false);
  const [minZoom, setMinZoom] = useState(0);
  const [maxZoom, setMaxZoom] = useState(22);
  const [zoomError, setZoomError] = useState('');
  const [isSavingZoom, setIsSavingZoom] = useState(false);
  const [editUseAliases, setEditUseAliases] = useState(false);
  const [useAliasesValue, setUseAliasesValue] = useState(true);
  const [useAliasesError, setUseAliasesError] = useState('');
  const [isSavingUseAliases, setIsSavingUseAliases] = useState(false);
  const [editingAlias, setEditingAlias] = useState(null);
  const [aliasValue, setAliasValue] = useState('');
  const [aliasError, setAliasError] = useState('');
  const [isSavingAlias, setIsSavingAlias] = useState(false);
  const aliasEditRef = useRef(null);

  // Iframe embed states
  const [iframeCodeExpanded, setIframeCodeExpanded] = useState(false);
  const [iframeWidth, setIframeWidth] = useState('100%');
  const [iframeHeight, setIframeHeight] = useState('400');
  const [copyIframeSuccess, setCopyIframeSuccess] = useState(false);

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
  const [publishMinZoomTouched, setPublishMinZoomTouched] = useState(false);
  const [publishMaxZoomTouched, setPublishMaxZoomTouched] = useState(false);
  const [publishUseAliases, setPublishUseAliases] = useState(true);
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
    const minZoom = publishMinZoom === '' ? 0 : publishMinZoom;
    const maxZoom = publishMaxZoom === '' ? 22 : publishMaxZoom;
    return minZoom > maxZoom ? '最小层级不能大于最大层级' : null;
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
      setEditUseAliases(false);
      setUseAliasesValue(file.useAliases ?? true);
      setUseAliasesError('');
      setEditingAlias(null);
      setAliasValue('');
      setAliasError('');
      // Reset publish-related state when file changes
      setEditPublish(false);
      setPublishSlug('');
      setPublishMinZoom(getInitialPublishMinZoom(file));
      setPublishMaxZoom(getInitialPublishMaxZoom(file));
      setPublishMinZoomTouched(false);
      setPublishMaxZoomTouched(false);
      setPublishUseAliases(true);
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

  function isPmtilesFile(fileItem) {
    return (
      fileItem?.tileSource === 'pmtiles' ||
      fileItem?.type === 'pmtiles' ||
      fileItem?.fileType === 'pmtiles'
    );
  }

  function getInitialPublishMinZoom(fileItem) {
    if (isPmtilesFile(fileItem) && fileItem?.minZoom == null) {
      return '';
    }
    return fileItem?.minZoom ?? 0;
  }

  function getInitialPublishMaxZoom(fileItem) {
    if (isPmtilesFile(fileItem) && fileItem?.maxZoom == null) {
      return '';
    }
    return fileItem?.maxZoom ?? 22;
  }

  function getPublicUrlPath(slug, fileItem) {
    if (!slug) {
      return '';
    }

    return isPmtilesFile(fileItem) ? `/tiles/${slug}` : `/tiles/${slug}/{z}/{x}/{y}`;
  }

  function copyPublicUrl(slug, fileItem) {
    if (!slug) {
      return;
    }
    const url = `${window.location.origin}${getPublicUrlPath(slug, fileItem)}`;
    navigator.clipboard
      .writeText(url)
      .then(() => {
        setCopySuccess(true);
        setTimeout(() => setCopySuccess(false), 2000);
      })
      .catch(() => {
        alert(t('file.detail.copyFailedManual'));
      });
  }

  // Generate iframe embed code
  function generateIframeCode() {
    if (!file?.publicSlug) return '';

    // Auto-add 'px' unit if user enters a plain number
    const formatDimension = (val, defaultVal) => {
      if (typeof val !== 'string') return defaultVal;
      const trimmed = val.trim();
      if (!trimmed) return defaultVal;
      // If it's a pure number, add 'px'
      if (/^\d+(\.\d+)?$/.test(trimmed)) {
        return `${trimmed}px`;
      }
      return trimmed;
    };

    const formattedWidth = formatDimension(iframeWidth, '100%');
    const formattedHeight = formatDimension(iframeHeight, '400px');
    const embedUrl = `${window.location.origin}/tiles/${file.publicSlug}/embed`;

    return `<iframe
  src="${embedUrl}"
  title="MapFlow map"
  loading="lazy"
  style="width:${formattedWidth};height:${formattedHeight};border:0;"
></iframe>`;
  }

  // Copy iframe embed code
  function handleCopyIframe() {
    const code = generateIframeCode();
    if (!code) {
      alert(t('file.detail.cannotGenerateEmbed'));
      return;
    }

    navigator.clipboard
      .writeText(code)
      .then(() => {
        setCopyIframeSuccess(true);
        setTimeout(() => setCopyIframeSuccess(false), 2000);
      })
      .catch(() => {
        alert('复制失败，请手动复制代码');
      });
  }

  const publicUrlPath = getPublicUrlPath(file?.publicSlug, file);

  async function handlePublishSubmit() {
    if (!file) return;

    setPublishError('');
    setIsPublishing(true);

    try {
      const options = {
        slug: publishSlug.trim() || undefined,
        useAliases: publishUseAliases,
      };
      if (!isTileFile) {
        if (isPmtilesFile(file)) {
          if (publishMinZoomTouched && publishMinZoom !== '') {
            options.minZoom = publishMinZoom;
          }
          if (publishMaxZoomTouched && publishMaxZoom !== '') {
            options.maxZoom = publishMaxZoom;
          }
        } else {
          options.minZoom = publishMinZoom === '' ? 0 : publishMinZoom;
          options.maxZoom = publishMaxZoom === '' ? 22 : publishMaxZoom;
        }
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
              data-testid={`detail-tab-${tab.id}`}
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
              <div className="detail-label">Source</div>
              <div className="detail-value">{file.tileSource || 'duckdb'}</div>
            </div>

            <div className="detail-group">
              <div className="detail-label">Size</div>
              <div className="detail-value">{formatSize(file.size || 0)}</div>
            </div>

            <div className="detail-group">
              <div className="detail-label">Status</div>
              <div className={`status ${file.status}`} data-testid="file-status">
                {getStatusLabel(t, file.status)}
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

                                  const handleCellKeyDown = (e) => {
                                    if (e.key === 'Enter' || e.key === ' ') {
                                      e.preventDefault();
                                      handleStartEdit();
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
                                        onKeyDown={!isEditing ? handleCellKeyDown : undefined}
                                        role={!isEditing ? 'button' : undefined}
                                        tabIndex={!isEditing ? 0 : undefined}
                                        aria-label={`编辑别名: ${field.alias || '未设置'}`}
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
                                                data-testid="alias-save-button"
                                              >
                                                {isSavingAlias ? '...' : '保存'}
                                              </button>
                                              <button
                                                type="button"
                                                className="btn-secondary"
                                                onClick={handleCancel}
                                                data-testid="alias-cancel-button"
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
                        data-testid="publish-button"
                        onClick={() => {
                          setPublishSlug('');
                          setPublishMinZoom(getInitialPublishMinZoom(file));
                          setPublishMaxZoom(getInitialPublishMaxZoom(file));
                          setPublishMinZoomTouched(false);
                          setPublishMaxZoomTouched(false);
                          setEditPublish(true);
                        }}
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
                                onChange={(e) => {
                                  const value = e.target.value;
                                  setPublishMinZoomTouched(true);
                                  if (isPmtilesFile(file) && value === '') {
                                    setPublishMinZoom('');
                                    return;
                                  }
                                  const parsed = Number.parseInt(value, 10);
                                  setPublishMinZoom(Number.isNaN(parsed) ? 0 : parsed);
                                }}
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
                                onChange={(e) => {
                                  const value = e.target.value;
                                  setPublishMaxZoomTouched(true);
                                  if (isPmtilesFile(file) && value === '') {
                                    setPublishMaxZoom('');
                                    return;
                                  }
                                  const parsed = Number.parseInt(value, 10);
                                  setPublishMaxZoom(Number.isNaN(parsed) ? 22 : parsed);
                                }}
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

                      {file.tileFormat == null && (
                        <div>
                          <label
                            style={{
                              fontSize: '12px',
                              color: '#666',
                              marginBottom: '4px',
                              display: 'block',
                            }}
                          >
                            字段名称
                          </label>
                          <div style={{ display: 'flex', gap: '12px', alignItems: 'center' }}>
                            <label
                              style={{
                                display: 'flex',
                                alignItems: 'center',
                                gap: '4px',
                                cursor: 'pointer',
                              }}
                            >
                              <input
                                type="radio"
                                name="useAliases"
                                checked={publishUseAliases}
                                onChange={() => setPublishUseAliases(true)}
                              />
                              <span style={{ fontSize: '12px' }}>使用别名</span>
                            </label>
                            <label
                              style={{
                                display: 'flex',
                                alignItems: 'center',
                                gap: '4px',
                                cursor: 'pointer',
                              }}
                            >
                              <input
                                type="radio"
                                name="useAliases"
                                checked={!publishUseAliases}
                                onChange={() => setPublishUseAliases(false)}
                              />
                              <span style={{ fontSize: '12px' }}>使用原始名称</span>
                            </label>
                          </div>
                          <small className="form-hint">控制公开发布瓦片中的属性字段名称</small>
                        </div>
                      )}

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
                          {t('file.detail.publicUrl')}
                        </label>
                        <div className="form-value code" style={{ fontSize: '12px' }}>
                          {getPublicUrlPath(publishSlug.trim() || file.id, file)}
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
                          data-testid="confirm-publish-button"
                        >
                          {isPublishing
                            ? t('file.detail.publishing')
                            : t('file.detail.confirmPublish')}
                        </button>
                        <button
                          type="button"
                          className="btn-secondary"
                          style={{ fontSize: '12px', padding: '4px 12px' }}
                          onClick={() => {
                            setEditPublish(false);
                            setPublishSlug('');
                            setPublishMinZoom(getInitialPublishMinZoom(file));
                            setPublishMaxZoom(getInitialPublishMaxZoom(file));
                            setPublishMinZoomTouched(false);
                            setPublishMaxZoomTouched(false);
                            setPublishError('');
                          }}
                        >
                          {t('common.cancel')}
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              )}

              {file.isPublic && (
                <>
                  <div className="detail-group">
                    <div className="detail-label">{t('file.detail.publishStatus')}</div>
                    <div className="detail-value">
                      <span style={{ color: '#4caf50' }} data-testid="published-status">
                        {t('file.detail.published')}
                      </span>
                    </div>
                  </div>

                  <div className="detail-group">
                    <div className="detail-label">{t('file.detail.publicUrl')}</div>
                    <div className="detail-value">
                      <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
                        <div className="form-value code" style={{ fontSize: '12px' }}>
                          {publicUrlPath}
                        </div>
                        <div style={{ display: 'flex', gap: '12px', alignItems: 'center' }}>
                          <button
                            type="button"
                            className="btn-text"
                            style={{ fontSize: '12px', padding: 0, textAlign: 'left' }}
                            onClick={() => copyPublicUrl(file.publicSlug, file)}
                            data-testid="copy-url-button"
                          >
                            {copySuccess ? t('common.copied') : t('file.detail.copyAddress')}
                          </button>
                          <a
                            href={`/tiles/${file.publicSlug}/docs`}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="btn-text"
                            style={{ fontSize: '12px', textDecoration: 'none' }}
                          >
                            {t('file.detail.viewDocs')}
                          </a>
                        </div>
                      </div>
                    </div>
                  </div>

                  <div className="detail-group">
                    <button
                      type="button"
                      className="detail-label"
                      style={{
                        width: '100%',
                        border: 'none',
                        background: 'none',
                        padding: 0,
                        textAlign: 'left',
                        cursor: 'pointer',
                        display: 'flex',
                        alignItems: 'center',
                        gap: '6px',
                      }}
                      aria-expanded={iframeCodeExpanded}
                      aria-controls="iframe-embed-panel"
                      onClick={() => setIframeCodeExpanded((prev) => !prev)}
                      data-testid="embed-code-toggle"
                    >
                      <span>{t('file.detail.embedCode')}</span>
                      <span style={{ fontSize: '10px', color: '#999' }}>
                        {iframeCodeExpanded ? '▼' : '▶'}
                      </span>
                    </button>
                    {iframeCodeExpanded && (
                      <div
                        id="iframe-embed-panel"
                        className="detail-value"
                        style={{ marginTop: '8px' }}
                      >
                        <div className="iframe-embed-section">
                          <div className="iframe-size-inputs">
                            <label>
                              {t('file.detail.iframeWidth')}
                              <input
                                type="text"
                                value={iframeWidth}
                                onChange={(e) => setIframeWidth(e.target.value)}
                                placeholder="100%"
                                className="form-input"
                                style={{ width: '70px', fontSize: '12px' }}
                              />
                            </label>
                            <label style={{ marginLeft: '12px' }}>
                              {t('file.detail.iframeHeight')}
                              <input
                                type="text"
                                value={iframeHeight}
                                onChange={(e) => setIframeHeight(e.target.value)}
                                placeholder="400"
                                className="form-input"
                                style={{ width: '70px', fontSize: '12px' }}
                              />
                            </label>
                          </div>

                          <pre className="iframe-code-preview">{generateIframeCode()}</pre>

                          <button
                            type="button"
                            className="btn-primary"
                            style={{
                              fontSize: '12px',
                              padding: '6px 12px',
                              marginTop: '8px',
                              width: '100%',
                            }}
                            onClick={handleCopyIframe}
                          >
                            {copyIframeSuccess
                              ? `✓ ${t('common.copied')}`
                              : t('file.detail.copyEmbedCode')}
                          </button>

                          <div className="iframe-mini-preview" style={{ marginTop: '12px' }}>
                            <div style={{ fontSize: '11px', color: '#888', marginBottom: '6px' }}>
                              {t('file.detail.previewEffect')}
                            </div>
                            <iframe
                              src={`/tiles/${file.publicSlug}/embed`}
                              title="MapFlow embed preview"
                              loading="lazy"
                              style={{
                                width: '100%',
                                height: '120px',
                                border: '1px solid #ddd',
                                borderRadius: '4px',
                                background: '#f5f4f2',
                              }}
                            />
                          </div>
                        </div>
                      </div>
                    )}
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

              {file.isPublic && file.tileFormat == null && (
                <div className="detail-group">
                  <div className="detail-label">字段名称</div>
                  <div className="detail-value">
                    {editUseAliases ? (
                      <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
                        <div style={{ display: 'flex', gap: '12px', alignItems: 'center' }}>
                          <label
                            style={{
                              display: 'flex',
                              alignItems: 'center',
                              gap: '4px',
                              cursor: 'pointer',
                            }}
                          >
                            <input
                              type="radio"
                              name="editUseAliases"
                              checked={useAliasesValue}
                              onChange={() => setUseAliasesValue(true)}
                            />
                            <span style={{ fontSize: '12px' }}>使用别名</span>
                          </label>
                          <label
                            style={{
                              display: 'flex',
                              alignItems: 'center',
                              gap: '4px',
                              cursor: 'pointer',
                            }}
                          >
                            <input
                              type="radio"
                              name="editUseAliases"
                              checked={!useAliasesValue}
                              onChange={() => setUseAliasesValue(false)}
                            />
                            <span style={{ fontSize: '12px' }}>使用原始名称</span>
                          </label>
                        </div>
                        <div style={{ display: 'flex', gap: '8px' }}>
                          <button
                            type="button"
                            className="btn-primary"
                            style={{ fontSize: '12px', padding: '4px 12px' }}
                            disabled={isSavingUseAliases}
                            onClick={async () => {
                              setUseAliasesError('');
                              setIsSavingUseAliases(true);
                              try {
                                await updatePublishSettings(file.id, {
                                  useAliases: useAliasesValue,
                                });
                                setEditUseAliases(false);
                                if (onUseAliasesUpdate) {
                                  onUseAliasesUpdate(file.id, useAliasesValue);
                                }
                              } catch (err) {
                                setUseAliasesError(err.message || '更新字段名称设置失败');
                              } finally {
                                setIsSavingUseAliases(false);
                              }
                            }}
                          >
                            {isSavingUseAliases ? '保存中...' : '保存'}
                          </button>
                          <button
                            type="button"
                            className="btn-secondary"
                            style={{ fontSize: '12px', padding: '4px 12px' }}
                            onClick={() => {
                              setUseAliasesValue(file.useAliases ?? true);
                              setEditUseAliases(false);
                              setUseAliasesError('');
                            }}
                          >
                            取消
                          </button>
                        </div>
                        {useAliasesError && (
                          <div className="alert" style={{ fontSize: '12px', margin: 0 }}>
                            {useAliasesError}
                          </div>
                        )}
                      </div>
                    ) : (
                      <div
                        style={{
                          display: 'flex',
                          justifyContent: 'space-between',
                          alignItems: 'center',
                        }}
                      >
                        <span>{file.useAliases !== false ? '使用别名' : '使用原始名称'}</span>
                        <button
                          type="button"
                          className="btn-text"
                          style={{ fontSize: '12px' }}
                          onClick={() => setEditUseAliases(true)}
                        >
                          修改
                        </button>
                      </div>
                    )}
                    <small className="form-hint" style={{ marginTop: '4px' }}>
                      控制公开发布瓦片中的属性字段名称
                    </small>
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
                    data-testid="unpublish-button"
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
  const { t } = useTranslation();
  const { user, logout } = useAuth();
  const [files, setFiles] = useState([]);
  const [selectedId, setSelectedId] = useState(null);
  const [errorMessage, setErrorMessage] = useState('');
  const [isLoading, setIsLoading] = useState(true);
  const [showPostgisModal, setShowPostgisModal] = useState(false);
  const [postgisForm, setPostgisForm] = useState(INITIAL_POSTGIS_FORM);
  const [postgisMessage, setPostgisMessage] = useState('');
  const [isTestingPostgis, setIsTestingPostgis] = useState(false);
  const [isRegisteringPostgis, setIsRegisteringPostgis] = useState(false);

  async function refreshFiles(nextSelectedId = null) {
    const res = await fetch('/api/files');
    const data = await res.json();
    setFiles(Array.isArray(data) ? data : []);
    if (nextSelectedId) {
      setSelectedId(nextSelectedId);
    }
  }

  function resetPostgisForm() {
    setPostgisForm(INITIAL_POSTGIS_FORM);
    setPostgisMessage('');
  }

  function updatePostgisField(field, value) {
    setPostgisForm((prev) => ({ ...prev, [field]: value }));
  }

  async function handleLogout() {
    try {
      await logout();
      window.location.href = '/login';
    } catch (error) {
      console.error('Logout failed:', error);
    }
  }

  async function handleTestPostgisConnection() {
    setPostgisMessage('');
    setIsTestingPostgis(true);
    try {
      const payload = {
        connection: {
          host: postgisForm.host.trim(),
          port: Number(postgisForm.port),
          database: postgisForm.database.trim(),
          username: postgisForm.username.trim(),
          password: postgisForm.password,
          sslMode: postgisForm.sslMode,
        },
      };
      const result = await testPostgisConnection(payload);
      setPostgisMessage(
        `连接成功: PostgreSQL ${result.serverVersion}, ${result.postgisVersion.split(' ').slice(0, 2).join(' ')}`,
      );
    } catch (error) {
      setPostgisMessage(error instanceof Error ? error.message : 'PostGIS 连接测试失败');
    } finally {
      setIsTestingPostgis(false);
    }
  }

  async function handleRegisterPostgisSource() {
    setPostgisMessage('');
    setIsRegisteringPostgis(true);
    try {
      const payload = {
        connectionName: postgisForm.connectionName.trim(),
        connection: {
          host: postgisForm.host.trim(),
          port: Number(postgisForm.port),
          database: postgisForm.database.trim(),
          username: postgisForm.username.trim(),
          password: postgisForm.password,
          sslMode: postgisForm.sslMode,
        },
        schema: postgisForm.schema.trim(),
        object: postgisForm.object.trim(),
        geometryColumn: postgisForm.geometryColumn.trim(),
        fidColumn: postgisForm.fidColumn.trim(),
        displayName: postgisForm.displayName.trim() || undefined,
      };
      const result = await registerPostgisSource(payload);
      await refreshFiles(result.fileId);
      setShowPostgisModal(false);
      resetPostgisForm();
    } catch (error) {
      setPostgisMessage(error instanceof Error ? error.message : 'PostGIS 数据源注册失败');
    } finally {
      setIsRegisteringPostgis(false);
    }
  }

  async function handlePublish(fileId, options) {
    const result = await publishFile(fileId, options);
    setFiles((prev) =>
      prev.map((f) =>
        f.id === fileId
          ? { ...f, isPublic: true, publicSlug: result.slug, useAliases: result.useAliases }
          : f,
      ),
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

  function handleUseAliasesUpdate(fileId, useAliases) {
    setFiles((prev) => prev.map((f) => (f.id === fileId ? { ...f, useAliases } : f)));
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
          <p className="subtitle">{t('app.subtitle')}</p>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: '1rem' }}>
          {user && (
            <span style={{ fontSize: '14px', color: '#666' }}>
              {user.username} ({user.role})
            </span>
          )}
          <LanguageSwitcher />
          {user?.role === 'admin' && (
            <a href="/settings" className="btn-text" style={{ fontSize: '14px' }}>
              {t('app.settings')}
            </a>
          )}
          {user?.role === 'admin' && (
            <button
              type="button"
              className="btn-secondary"
              onClick={() => {
                setShowPostgisModal(true);
                setPostgisMessage('');
              }}
            >
              {t('app.connectPostgis')}
            </button>
          )}
          <label className="upload-button">
            <input
              type="file"
              accept=".zip,.geojson,.json,.geojsonl,.geojsons,.kml,.gpx,.topojson,.mbtiles,.pmtiles"
              onChange={handleFileChange}
              data-testid="file-input"
            />
            {t('app.upload')}
          </label>
          {user && (
            <button
              type="button"
              className="btn-secondary"
              onClick={handleLogout}
              data-testid="logout-button"
            >
              {t('app.logout')}
            </button>
          )}
        </div>
      </header>

      {errorMessage ? <div className="alert">{errorMessage}</div> : null}

      <section className="panel">
        <div className="panel-header">
          <h2>{t('app.dataSource')}</h2>
          <span className="panel-meta">{t('app.supportedFormatsHint')}</span>
        </div>

        <div className="panel-body">
          <div className="list-area">
            {isLoading ? (
              <div className="empty">{t('app.loading')}</div>
            ) : orderedFiles.length === 0 ? (
              <div className="empty" data-testid="empty-state">
                {t('app.noFiles')}
              </div>
            ) : (
              <div className="table">
                <div className="row head">
                  <div>{t('fileList.name')}</div>
                  <div>{t('fileList.type')}</div>
                  <div>{t('fileList.size')}</div>
                  <div>{t('fileList.uploadTime')}</div>
                  <div>{t('fileList.status')}</div>
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
                    <div>
                      {item.type}
                      {item.tileSource === 'postgis' ? ' · PostGIS' : ''}
                    </div>
                    <div>{formatSize(item.size || 0)}</div>
                    <div className="muted">
                      {item.uploadedAt ? new Date(item.uploadedAt).toLocaleString() : '--'}
                    </div>
                    <div
                      className={`status ${item.status || 'uploaded'}`}
                      data-testid={`status-${item.status || 'uploaded'}`}
                    >
                      {getStatusLabel(t, item.status)}
                    </div>
                    <div onClick={(e) => e.stopPropagation()}>
                      {item.status === 'ready' && (
                        <a
                          href={`/preview/${item.id}`}
                          target="_blank"
                          rel="noopener noreferrer"
                          className="btn-text"
                          title={t('fileList.viewInNewWindow')}
                          data-testid="preview-link"
                        >
                          {t('fileList.view')}
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
              onUseAliasesUpdate={handleUseAliasesUpdate}
            />
          </div>
        </div>
      </section>

      {showPostgisModal && (
        <div className="modal-overlay" onClick={() => setShowPostgisModal(false)}>
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h3>{t('postgis.connect')}</h3>
              <button
                type="button"
                className="modal-close"
                onClick={() => setShowPostgisModal(false)}
                aria-label="Close"
              >
                ×
              </button>
            </div>
            <div className="modal-body">
              <div className="form-group">
                <label>Connection Name</label>
                <input
                  className="form-input"
                  value={postgisForm.connectionName}
                  onChange={(e) => updatePostgisField('connectionName', e.target.value)}
                  placeholder="local-dev"
                />
              </div>
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 120px', gap: '12px' }}>
                <div className="form-group">
                  <label>Host</label>
                  <input
                    className="form-input"
                    value={postgisForm.host}
                    onChange={(e) => updatePostgisField('host', e.target.value)}
                  />
                </div>
                <div className="form-group">
                  <label>Port</label>
                  <input
                    className="form-input"
                    type="number"
                    value={postgisForm.port}
                    onChange={(e) => updatePostgisField('port', Number(e.target.value))}
                  />
                </div>
              </div>
              <div className="form-group">
                <label>Database</label>
                <input
                  className="form-input"
                  value={postgisForm.database}
                  onChange={(e) => updatePostgisField('database', e.target.value)}
                />
              </div>
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '12px' }}>
                <div className="form-group">
                  <label>Username</label>
                  <input
                    className="form-input"
                    value={postgisForm.username}
                    onChange={(e) => updatePostgisField('username', e.target.value)}
                  />
                </div>
                <div className="form-group">
                  <label>Password</label>
                  <input
                    className="form-input"
                    type="password"
                    value={postgisForm.password}
                    onChange={(e) => updatePostgisField('password', e.target.value)}
                  />
                </div>
              </div>
              <div className="form-group">
                <label>Schema</label>
                <input
                  className="form-input"
                  value={postgisForm.schema}
                  onChange={(e) => updatePostgisField('schema', e.target.value)}
                />
              </div>
              <div className="form-group">
                <label>Table/View</label>
                <input
                  className="form-input"
                  value={postgisForm.object}
                  onChange={(e) => updatePostgisField('object', e.target.value)}
                  placeholder="roads"
                />
              </div>
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '12px' }}>
                <div className="form-group">
                  <label>Geometry Column</label>
                  <input
                    className="form-input"
                    value={postgisForm.geometryColumn}
                    onChange={(e) => updatePostgisField('geometryColumn', e.target.value)}
                  />
                </div>
                <div className="form-group">
                  <label>FID Column</label>
                  <input
                    className="form-input"
                    value={postgisForm.fidColumn}
                    onChange={(e) => updatePostgisField('fidColumn', e.target.value)}
                  />
                </div>
              </div>
              <div className="form-group">
                <label>Display Name (Optional)</label>
                <input
                  className="form-input"
                  value={postgisForm.displayName}
                  onChange={(e) => updatePostgisField('displayName', e.target.value)}
                />
              </div>
              {postgisMessage ? (
                <div
                  className="form-hint"
                  style={{
                    color: postgisMessage.startsWith('连接成功') ? '#2e7d32' : '#c62828',
                    marginTop: '8px',
                  }}
                >
                  {postgisMessage}
                </div>
              ) : null}
            </div>
            <div className="modal-footer">
              <button
                type="button"
                className="btn-secondary"
                onClick={handleTestPostgisConnection}
                disabled={isTestingPostgis || isRegisteringPostgis}
              >
                {isTestingPostgis ? '测试中...' : '测试连接'}
              </button>
              <button
                type="button"
                className="upload-button"
                onClick={handleRegisterPostgisSource}
                disabled={isTestingPostgis || isRegisteringPostgis}
              >
                {isRegisteringPostgis ? '注册中...' : '注册为数据源'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
