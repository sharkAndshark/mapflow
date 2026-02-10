import React, { useEffect, useMemo, useState } from 'react';
import { useAuth } from './AuthContext.jsx';
import {
  hasActiveJobs as computeHasActiveJobs,
  mergeServerFilesWithOptimistic,
} from './polling.js';
import { publishFile, unpublishFile } from './api.js';

function PublishModal({ file, onClose, onSuccess }) {
  const [slug, setSlug] = useState(file?.id || '');
  const [error, setError] = useState('');
  const [isSubmitting, setIsSubmitting] = useState(false);

  if (!file) return null;

  const handleSubmit = async (e) => {
    e.preventDefault();
    setError('');
    setIsSubmitting(true);

    try {
      const result = await publishFile(file.id, slug.trim() || undefined);
      onSuccess(file.id, result);
    } catch (err) {
      setError(err.message || '发布失败');
    } finally {
      setIsSubmitting(false);
    }
  };

  useEffect(() => {
    const handleEscape = (e) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handleEscape);
    return () => window.removeEventListener('keydown', handleEscape);
  }, [onClose]);

  const trimmedSlug = slug.trim();
  const previewUrl = trimmedSlug
    ? `/tiles/${trimmedSlug}/{z}/{x}/{y}`
    : `/tiles/${file.id}/{z}/{x}/{y}`;
  const slugError = !trimmedSlug
    ? 'URL 标识不能为空或仅包含空格'
    : trimmedSlug.length > 100
      ? 'URL 标识不能超过 100 个字符'
      : !/^[a-zA-Z0-9_-]+$/.test(trimmedSlug)
        ? '仅支持字母、数字、连字符和下划线'
        : '';

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal-content" onClick={(e) => e.stopPropagation()}>
        <div className="modal-header">
          <h3>发布文件</h3>
          <button className="modal-close" onClick={onClose} aria-label="关闭">
            ×
          </button>
        </div>
        <form onSubmit={handleSubmit}>
          <div className="modal-body">
            <div className="form-group">
              <label>文件名</label>
              <div className="form-value">{file.name}</div>
            </div>
            <div className="form-group">
              <label htmlFor="slug">URL 标识（可选）</label>
              <input
                id="slug"
                type="text"
                value={slug}
                onChange={(e) => setSlug(e.target.value)}
                placeholder={file.id}
                className="form-input"
              />
              {slugError && (
                <div className="alert" style={{ marginTop: '8px' }}>
                  {slugError}
                </div>
              )}
              <small className="form-hint">
                留空则使用文件 ID。仅支持字母、数字、连字符和下划线
              </small>
            </div>
            {previewUrl && (
              <div className="form-group">
                <label>公开地址</label>
                <div className="form-value code">{previewUrl}</div>
              </div>
            )}
            {error && <div className="alert">{error}</div>}
          </div>
          <div className="modal-footer">
            <button type="button" className="btn-secondary" onClick={onClose}>
              取消
            </button>
            <button type="submit" className="btn-primary" disabled={isSubmitting || !!slugError}>
              {isSubmitting ? '发布中...' : '确认发布'}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}

const STATUS_LABELS = {
  uploading: '上传中',
  uploaded: '等待处理',
  processing: '处理中',
  ready: '已就绪',
  failed: '失败',
};

function formatSize(bytes) {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / Math.pow(1024, index);
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[index]}`;
}

function parseType(fileName) {
  const lower = fileName.toLowerCase();
  if (lower.endsWith('.zip')) return 'shapefile';
  if (lower.endsWith('.geojson') || lower.endsWith('.json')) return 'geojson';
  if (lower.endsWith('.geojsonl') || lower.endsWith('.geojsons')) return 'geojsonl';
  if (lower.endsWith('.kml')) return 'kml';
  if (lower.endsWith('.gpx')) return 'gpx';
  if (lower.endsWith('.topojson')) return 'topojson';
  return 'unknown';
}

function DetailSidebar({ file }) {
  const [schema, setSchema] = useState(null);
  const [schemaError, setSchemaError] = useState(null);
  const [isLoadingSchema, setIsLoadingSchema] = useState(false);

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

  if (!file) {
    return (
      <div className="detail-empty">
        <p>选择一个文件查看详情</p>
      </div>
    );
  }

  const isReady = file.status === 'ready';
  const isFailed = file.status === 'failed';
  const canPreview = isReady;

  return (
    <div className="detail-content" data-testid="detail-sidebar">
      <div className="detail-header">
        <h3 className="detail-title">{file.name}</h3>
        <span className="detail-id">{file.id}</span>
      </div>

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

      {isReady && (
        <div className="detail-group">
          <div className="detail-label">字段信息</div>
          <div className="detail-value">
            {isLoadingSchema ? (
              <span style={{ color: '#888', fontSize: '12px' }}>加载中...</span>
            ) : schemaError ? (
              <span style={{ color: '#d32f2f', fontSize: '12px' }}>{schemaError}</span>
            ) : schema?.fields ? (
              <div style={{ fontSize: '13px' }}>
                {schema.fields.length === 0 ? (
                  <span style={{ color: '#888' }}>无字段</span>
                ) : (
                  <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
                    {schema.fields.map((field) => (
                      <div
                        key={field.name}
                        style={{
                          display: 'flex',
                          justifyContent: 'space-between',
                          alignItems: 'center',
                          padding: '2px 0',
                        }}
                      >
                        <span style={{ fontWeight: 500 }}>{field.name}</span>
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
                      </div>
                    ))}
                  </div>
                )}
              </div>
            ) : null}
          </div>
        </div>
      )}

      {isFailed && file.error && (
        <div className="detail-error">
          <strong>Error:</strong> {file.error}
        </div>
      )}

      <div className="detail-actions">
        {canPreview ? (
          <a
            href={`/preview/${file.id}`}
            target="_blank"
            rel="noopener noreferrer"
            className="btn-primary"
            data-testid="open-preview"
          >
            Open Preview
          </a>
        ) : (
          <span className="btn-primary disabled" aria-disabled="true">
            Open Preview
          </span>
        )}
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
  const [publishModalFile, setPublishModalFile] = useState(null);

  async function handleLogout() {
    try {
      await logout();
      window.location.href = '/login';
    } catch (error) {
      console.error('Logout failed:', error);
    }
  }

  async function handlePublish(file) {
    setPublishModalFile(file);
  }

  async function handlePublishSuccess(fileId, result) {
    setPublishModalFile(null);
    setFiles((prev) =>
      prev.map((f) => (f.id === fileId ? { ...f, isPublic: true, publicSlug: result.slug } : f)),
    );
  }

  async function handleUnpublish(file) {
    if (!confirm(`确定取消发布 "${file.name}" 吗？`)) return;

    try {
      await unpublishFile(file.id);
      setFiles((prev) =>
        prev.map((f) => (f.id === file.id ? { ...f, isPublic: false, publicSlug: null } : f)),
      );
    } catch (err) {
      setErrorMessage(err.message || '取消发布失败');
    }
  }

  function copyPublicUrl(slug) {
    if (!slug) {
      alert('无效的公开地址');
      return;
    }
    const url = `${window.location.origin}/tiles/${slug}/{z}/{x}/{y}`;
    navigator.clipboard
      .writeText(url)
      .then(() => {
        alert('已复制到剪贴板');
      })
      .catch(() => {
        alert('复制失败，请手动复制地址');
      });
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
          <p className="subtitle">探索版 · 文件上传与列表</p>
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
              accept=".zip,.geojson,.json,.geojsonl,.geojsons,.kml,.gpx,.topojson"
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
            支持 .zip / .geojson / .geojsonl / .kml / .gpx / .topojson，单文件最大 200MB（可配置）
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
                  <button
                    key={item.id}
                    type="button"
                    className={`row ${selectedId === item.id ? 'selected' : ''}`}
                    onClick={() => setSelectedId(item.id)}
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
                      {item.status === 'ready' ? (
                        item.isPublic ? (
                          <>
                            <button
                              type="button"
                              className="btn-text"
                              onClick={() => copyPublicUrl(item.publicSlug)}
                              title="复制地址"
                            >
                              复制
                            </button>
                            <button
                              type="button"
                              className="btn-text"
                              onClick={() => handleUnpublish(item)}
                              title="取消发布"
                            >
                              取消发布
                            </button>
                          </>
                        ) : (
                          <button
                            type="button"
                            className="btn-text"
                            onClick={() => handlePublish(item)}
                            title="发布"
                          >
                            发布
                          </button>
                        )
                      ) : null}
                    </div>
                  </button>
                ))}
              </div>
            )}
          </div>

          <div className="detail-area">
            <DetailSidebar file={selectedFile} />
          </div>
        </div>
      </section>

      {publishModalFile && (
        <PublishModal
          file={publishModalFile}
          onClose={() => setPublishModalFile(null)}
          onSuccess={handlePublishSuccess}
        />
      )}
    </div>
  );
}
