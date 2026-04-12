import React, { useCallback, useEffect, useState, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { listIcons, uploadIcon, deleteIcon, updateIcon } from './api.js';

function formatBytes(bytes) {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

export default function IconsPanel() {
  const { t, i18n } = useTranslation();
  const [icons, setIcons] = useState([]);
  const [isLoading, setIsLoading] = useState(true);
  const [selectedId, setSelectedId] = useState(null);
  const [errorMessage, setErrorMessage] = useState('');
  const [isUploading, setIsUploading] = useState(false);
  const [editingName, setEditingName] = useState('');
  const [isSavingName, setIsSavingName] = useState(false);
  const fileInputRef = useRef(null);

  const dateTimeFormatter = useMemo(
    () =>
      new Intl.DateTimeFormat(i18n.resolvedLanguage || i18n.language || undefined, {
        dateStyle: 'medium',
        timeStyle: 'short',
      }),
    [i18n.language, i18n.resolvedLanguage],
  );

  const selectedIcon = icons.find((i) => i.id === selectedId);

  const refreshIcons = useCallback(async () => {
    try {
      const data = await listIcons();
      setIcons(Array.isArray(data) ? data : []);
      setErrorMessage('');
    } catch (err) {
      setErrorMessage(err.message || t('icon.loadFailed'));
    } finally {
      setIsLoading(false);
    }
  }, [t]);

  useEffect(() => {
    refreshIcons();
  }, [refreshIcons]);

  useEffect(() => {
    if (selectedIcon) {
      setEditingName(selectedIcon.name);
    }
  }, [selectedIcon]);

  async function handleFileChange(e) {
    const file = e.target.files[0];
    if (!file) return;

    const ext = file.name.split('.').pop().toLowerCase();
    if (!['png', 'svg'].includes(ext)) {
      setErrorMessage(t('icon.unsupportedFormat'));
      e.target.value = '';
      return;
    }

    setIsUploading(true);
    setErrorMessage('');
    try {
      await uploadIcon(file);
      await refreshIcons();
    } catch (err) {
      setErrorMessage(err.message || t('icon.uploadFailed'));
    } finally {
      setIsUploading(false);
      e.target.value = '';
    }
  }

  async function handleDelete(iconId) {
    if (!confirm(t('icon.deleteConfirm'))) return;
    try {
      await deleteIcon(iconId);
      if (selectedId === iconId) {
        setSelectedId(null);
      }
      await refreshIcons();
    } catch (err) {
      setErrorMessage(err.message || t('icon.deleteFailed'));
    }
  }

  async function handleSaveName() {
    if (!selectedIcon) return;
    const trimmed = editingName.trim();
    if (!trimmed) {
      setErrorMessage(t('icon.nameEmpty'));
      return;
    }
    if (trimmed === selectedIcon.name) return;

    setIsSavingName(true);
    setErrorMessage('');
    try {
      await updateIcon(selectedIcon.id, { name: trimmed });
      await refreshIcons();
    } catch (err) {
      setErrorMessage(err.message || t('icon.updateFailed'));
    } finally {
      setIsSavingName(false);
    }
  }

  function handleKeyDown(e) {
    if (e.key === 'Enter') {
      e.preventDefault();
      handleSaveName();
    }
  }

  return (
    <div className="panel-body">
      {errorMessage && (
        <div className="alert" onClick={() => setErrorMessage('')} style={{ cursor: 'pointer' }}>
          {errorMessage}
        </div>
      )}

      <div className="list-area">
        <div style={{ padding: '12px', borderBottom: '1px solid #e0e0e0' }}>
          <label className="upload-button" style={{ fontSize: '13px' }}>
            <input
              ref={fileInputRef}
              type="file"
              accept=".png,.svg"
              onChange={handleFileChange}
              disabled={isUploading}
              data-testid="icon-file-input"
            />
            {isUploading ? t('icon.uploading') : t('icon.uploadIcon')}
          </label>
        </div>

        {isLoading ? (
          <div className="empty">{t('common.loading')}</div>
        ) : icons.length === 0 ? (
          <div className="empty" data-testid="icons-empty-state">
            {t('icon.noIcons')}
          </div>
        ) : (
          <div className="icon-grid">
            {icons.map((icon) => (
              <button
                key={icon.id}
                type="button"
                className={`icon-card ${selectedId === icon.id ? 'selected' : ''}`}
                onClick={() => setSelectedId(icon.id)}
                data-testid={`icon-card-${icon.id}`}
              >
                <div className="icon-thumbnail">
                  <img
                    src={`/api/icons/${icon.id}/file`}
                    alt={icon.name}
                    loading="lazy"
                  />
                </div>
                <div className="icon-card-name" title={icon.name}>
                  {icon.name}
                </div>
              </button>
            ))}
          </div>
        )}
      </div>

      <div className="detail-area">
        {!selectedIcon ? (
          <div className="detail-empty">
            <p>{t('icon.selectIconToView')}</p>
          </div>
        ) : (
          <div className="detail-sidebar" data-testid="icon-detail-sidebar">
            <div className="detail-content">
              <div className="detail-header">
                <h3 className="detail-title">{selectedIcon.name}</h3>
                <span className="detail-id">{selectedIcon.id}</span>
              </div>

              <div className="detail-group">
                <div className="detail-preview">
                  <img
                    src={`/api/icons/${selectedIcon.id}/file`}
                    alt={selectedIcon.name}
                    style={{ maxWidth: '100%', maxHeight: '200px', objectFit: 'contain' }}
                  />
                </div>
              </div>

              <div className="detail-group">
                <div className="detail-label">{t('icon.name')}</div>
                <div className="detail-value" style={{ display: 'flex', gap: '8px', alignItems: 'center' }}>
                  <input
                    type="text"
                    value={editingName}
                    onChange={(e) => setEditingName(e.target.value)}
                    onKeyDown={handleKeyDown}
                    style={{
                      flex: 1,
                      padding: '4px 8px',
                      border: '1px solid #ddd',
                      borderRadius: '4px',
                      fontSize: '13px',
                    }}
                    data-testid="icon-name-input"
                  />
                  <button
                    type="button"
                    className="btn-text"
                    style={{ fontSize: '12px', whiteSpace: 'nowrap' }}
                    disabled={isSavingName || editingName.trim() === selectedIcon.name}
                    onClick={handleSaveName}
                    data-testid="icon-save-name"
                  >
                    {isSavingName ? t('common.saving') : t('common.save')}
                  </button>
                </div>
              </div>

              <div className="detail-group">
                <div className="detail-label">{t('icon.fileType')}</div>
                <div className="detail-value" style={{ textTransform: 'uppercase' }}>
                  {selectedIcon.fileType}
                </div>
              </div>

              {selectedIcon.width != null && selectedIcon.height != null && (
                <div className="detail-group">
                  <div className="detail-label">{t('icon.dimensions')}</div>
                  <div className="detail-value">
                    {selectedIcon.width} × {selectedIcon.height}
                  </div>
                </div>
              )}

              <div className="detail-group">
                <div className="detail-label">{t('icon.fileSize')}</div>
                <div className="detail-value">{formatBytes(selectedIcon.size)}</div>
              </div>

              <div className="detail-group">
                <div className="detail-label">{t('icon.uploadTime')}</div>
                <div className="detail-value">
                  {selectedIcon.createdAt
                    ? dateTimeFormatter.format(new Date(selectedIcon.createdAt))
                    : '-'}
                </div>
              </div>

              {selectedIcon.updatedAt && (
                <div className="detail-group">
                  <div className="detail-label">{t('icon.updateTime')}</div>
                  <div className="detail-value">
                    {dateTimeFormatter.format(new Date(selectedIcon.updatedAt))}
                  </div>
                </div>
              )}

              <div
                style={{ marginTop: 'auto', paddingTop: '16px', borderTop: '1px solid #e0e0e0' }}
              >
                <button
                  type="button"
                  className="btn-secondary"
                  style={{ fontSize: '12px', padding: '4px 12px', color: '#d32f2f' }}
                  onClick={() => handleDelete(selectedIcon.id)}
                  data-testid="icon-delete-button"
                >
                  {t('icon.deleteBtn')}
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
