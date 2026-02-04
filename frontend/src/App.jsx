import React, { useEffect, useMemo, useState } from 'react';

const STATUS_LABELS = {
  uploading: '上传中',
  uploaded: '已上传',
  failed: '失败'
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
  if (lower.endsWith('.geojson')) return 'geojson';
  return 'unknown';
}

export default function App() {
  const [files, setFiles] = useState([]);
  const [selectedId, setSelectedId] = useState(null);
  const [errorMessage, setErrorMessage] = useState('');
  const [isLoading, setIsLoading] = useState(true);

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
      crs: null
    };

    setFiles((prev) => [optimistic, ...prev]);

    const formData = new FormData();
    formData.append('file', file);

    try {
      const res = await fetch('/api/uploads', {
        method: 'POST',
        body: formData
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
          item.id === tempId ? { ...item, status: 'failed', error: message } : item
        )
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
        <label className="upload-button">
          <input
            type="file"
            accept=".zip,.geojson"
            onChange={handleFileChange}
            data-testid="file-input"
          />
          上传
        </label>
      </header>

      {errorMessage ? <div className="alert">{errorMessage}</div> : null}

      <section className="panel">
        <div className="panel-header">
          <h2>上传文件</h2>
          <span className="panel-meta">支持 .zip / .geojson，单文件最大 200MB</span>
        </div>

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
            </div>
            {orderedFiles.map((item) => (
              <button
                type="button"
                key={item.id}
                className={`row ${selectedId === item.id ? 'selected' : ''}`}
                onClick={() => setSelectedId(item.id)}
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
              </button>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
