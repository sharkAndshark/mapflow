import React, { useCallback, useEffect, useState, useMemo, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { listFonts, uploadFont, deleteFont, publishFont, unpublishFont } from './api.js';
import { validateSlug } from './utils.js';

const FONT_STATUS_LABELS = {
  processing: 'font.status.processing',
  ready: 'font.status.ready',
  failed: 'font.status.failed',
};

function getFontStatusLabel(t, status) {
  const key = FONT_STATUS_LABELS[status];
  return key ? t(key) : status;
}

export default function FontsPanel() {
  const { t, i18n } = useTranslation();
  const [fonts, setFonts] = useState([]);
  const [isLoading, setIsLoading] = useState(true);
  const [selectedId, setSelectedId] = useState(null);
  const [errorMessage, setErrorMessage] = useState('');
  const [isUploading, setIsUploading] = useState(false);
  const [publishSlug, setPublishSlug] = useState('');
  const [publishError, setPublishError] = useState('');
  const [isPublishing, setIsPublishing] = useState(false);
  const [copySuccess, setCopySuccess] = useState(false);
  const fileInputRef = useRef(null);

  const dateTimeFormatter = useMemo(
    () =>
      new Intl.DateTimeFormat(i18n.resolvedLanguage || i18n.language || undefined, {
        dateStyle: 'medium',
        timeStyle: 'short',
      }),
    [i18n.language, i18n.resolvedLanguage],
  );

  const selectedFont = fonts.find((f) => f.id === selectedId);

  const refreshFonts = useCallback(async () => {
    try {
      const data = await listFonts();
      setFonts(Array.isArray(data) ? data : []);
      setErrorMessage('');
    } catch (err) {
      setErrorMessage(err.message || t('font.loadFailed'));
    } finally {
      setIsLoading(false);
    }
  }, [t]);

  useEffect(() => {
    refreshFonts();
  }, [refreshFonts]);

  const hasProcessingFonts = useMemo(
    () => fonts.some((font) => font.status === 'processing'),
    [fonts],
  );

  useEffect(() => {
    if (!hasProcessingFonts) return;

    const interval = setInterval(() => {
      refreshFonts();
    }, 2000);

    return () => clearInterval(interval);
  }, [hasProcessingFonts, refreshFonts]);

  async function handleFileChange(e) {
    const file = e.target.files[0];
    if (!file) return;

    const ext = file.name.split('.').pop().toLowerCase();
    if (!['ttf', 'otf'].includes(ext)) {
      setErrorMessage(t('font.unsupportedFormat'));
      e.target.value = '';
      return;
    }

    setIsUploading(true);
    setErrorMessage('');
    try {
      await uploadFont(file);
      await refreshFonts();
    } catch (err) {
      setErrorMessage(err.message || t('font.uploadFailed'));
    } finally {
      setIsUploading(false);
      e.target.value = '';
    }
  }

  async function handleDelete(fontId) {
    if (!confirm(t('font.deleteConfirm'))) return;
    try {
      await deleteFont(fontId);
      if (selectedId === fontId) {
        setSelectedId(null);
      }
      await refreshFonts();
    } catch (err) {
      setErrorMessage(err.message || t('font.deleteFailed'));
    }
  }

  async function handlePublish(fontId) {
    setPublishError('');
    setIsPublishing(true);
    try {
      await publishFont(fontId, { slug: publishSlug.trim() || undefined });
      setPublishSlug('');
      await refreshFonts();
    } catch (err) {
      setPublishError(err.message || t('font.publishFailed'));
    } finally {
      setIsPublishing(false);
    }
  }

  async function handleUnpublish(fontId) {
    if (!confirm(t('font.unpublishConfirm'))) return;
    try {
      await unpublishFont(fontId);
      await refreshFonts();
    } catch (err) {
      setErrorMessage(err.message || t('font.unpublishFailed'));
    }
  }

  function copyPublicUrl(slug) {
    const url = `${window.location.origin}/fonts/${slug}/glyphs/{fontstack}/{range}.pbf`;
    navigator.clipboard
      .writeText(url)
      .then(() => {
        setCopySuccess(true);
        setTimeout(() => setCopySuccess(false), 2000);
      })
      .catch(() => {
        alert(t('font.copyFailed'));
      });
  }

  const slugValidationError = useMemo(() => {
    return validateSlug(publishSlug.trim(), {
      tooLong: t('file.detail.slugTooLong'),
      invalidChars: t('file.detail.slugInvalidChars'),
    }).error;
  }, [publishSlug, t]);

  return (
    <div className="panel-body">
      {errorMessage && <div className="alert">{errorMessage}</div>}

      <div className="list-area">
        <div style={{ padding: '12px', borderBottom: '1px solid #e0e0e0' }}>
          <label className="upload-button" style={{ fontSize: '13px' }}>
            <input
              ref={fileInputRef}
              type="file"
              accept=".ttf,.otf"
              onChange={handleFileChange}
              disabled={isUploading}
              data-testid="font-file-input"
            />
            {isUploading ? t('font.uploading') : t('font.uploadFont')}
          </label>
        </div>

        {isLoading ? (
          <div className="empty">{t('common.loading')}</div>
        ) : fonts.length === 0 ? (
          <div className="empty" data-testid="fonts-empty-state">
            {t('font.noFonts')}
          </div>
        ) : (
          <div className="table">
            <div className="row head">
              <div>{t('font.name')}</div>
              <div>{t('font.family')}</div>
              <div>{t('font.glyphCount')}</div>
              <div>{t('font.statusLabel')}</div>
              <div></div>
            </div>
            {fonts.map((font) => (
              <button
                key={font.id}
                type="button"
                className={`row ${selectedId === font.id ? 'selected' : ''}`}
                onClick={() => setSelectedId(font.id)}
                aria-pressed={selectedId === font.id}
                data-testid={`font-row-${font.id}`}
                style={{ width: '100%' }}
              >
                <span>{font.name}</span>
                <span>{font.family || '-'}</span>
                <span>{font.glyphCount || '-'}</span>
                <span>
                  <span className={`status ${font.status}`}>
                    {getFontStatusLabel(t, font.status)}
                  </span>
                </span>
                <span></span>
              </button>
            ))}
          </div>
        )}
      </div>

      <div className="detail-area">
        {!selectedFont ? (
          <div className="detail-empty">
            <p>{t('font.selectFontToView')}</p>
          </div>
        ) : (
          <div className="detail-sidebar" data-testid="font-detail-sidebar">
            <div className="detail-content">
              <div className="detail-header">
                <h3 className="detail-title">{selectedFont.name}</h3>
                <span className="detail-id">{selectedFont.id}</span>
              </div>

              <div className="detail-group">
                <div className="detail-label">{t('font.fontstack')}</div>
                <div className="detail-value">{selectedFont.fontstack}</div>
              </div>

              {selectedFont.family && (
                <div className="detail-group">
                  <div className="detail-label">{t('font.family')}</div>
                  <div className="detail-value">{selectedFont.family}</div>
                </div>
              )}

              {selectedFont.style && (
                <div className="detail-group">
                  <div className="detail-label">{t('font.style')}</div>
                  <div className="detail-value">{selectedFont.style}</div>
                </div>
              )}

              {selectedFont.glyphCount && (
                <div className="detail-group">
                  <div className="detail-label">{t('font.glyphCount')}</div>
                  <div className="detail-value">{selectedFont.glyphCount.toLocaleString()}</div>
                </div>
              )}

              {selectedFont.startCp != null && selectedFont.endCp != null && (
                <div className="detail-group">
                  <div className="detail-label">{t('font.unicodeRange')}</div>
                  <div className="detail-value">
                    U+{selectedFont.startCp.toString(16).toUpperCase().padStart(4, '0')} - U+
                    {selectedFont.endCp.toString(16).toUpperCase().padStart(4, '0')}
                  </div>
                </div>
              )}

              <div className="detail-group">
                <div className="detail-label">{t('font.statusLabel')}</div>
                <div className={`status ${selectedFont.status}`} data-testid="font-status">
                  {getFontStatusLabel(t, selectedFont.status)}
                </div>
              </div>

              <div className="detail-group">
                <div className="detail-label">{t('font.uploadTime')}</div>
                <div className="detail-value">
                  {selectedFont.createdAt
                    ? dateTimeFormatter.format(new Date(selectedFont.createdAt))
                    : '-'}
                </div>
              </div>

              {selectedFont.status === 'failed' && selectedFont.error && (
                <div className="detail-error">
                  <strong>{t('font.errorLabel')}:</strong> {selectedFont.error}
                </div>
              )}

              {selectedFont.status === 'ready' && (
                <>
                  {!selectedFont.isPublic ? (
                    <div className="detail-group">
                      <div className="detail-label">{t('font.publishStatus')}</div>
                      <div className="detail-value">
                        <div style={{ display: 'flex', flexDirection: 'column', gap: '12px' }}>
                          <span style={{ color: '#888' }}>{t('font.notPublished')}</span>

                          <div>
                            <label
                              htmlFor="font-publish-slug-input"
                              style={{
                                fontSize: '12px',
                                color: '#666',
                                marginBottom: '4px',
                                display: 'block',
                              }}
                            >
                              {t('font.publishSlug')}
                            </label>
                            <input
                              id="font-publish-slug-input"
                              type="text"
                              value={publishSlug}
                              onChange={(e) => setPublishSlug(e.target.value)}
                              placeholder={selectedFont.id}
                              className="form-input"
                              style={{ width: '100%' }}
                              data-testid="font-publish-slug-input"
                            />
                            {slugValidationError && (
                              <div className="alert" style={{ marginTop: '4px', fontSize: '12px' }}>
                                {slugValidationError}
                              </div>
                            )}
                            <small className="form-hint">{t('font.publishSlugHint')}</small>
                          </div>

                          <div>
                            <div style={{ fontSize: '12px', color: '#666', marginBottom: '4px' }}>
                              {t('font.publicUrl')}
                            </div>
                            <div className="form-value code" style={{ fontSize: '12px' }}>
                              /fonts/{publishSlug.trim() || selectedFont.id}/glyphs/{'{fontstack}'}/
                              {'{range}'}.pbf
                            </div>
                          </div>

                          {publishError && (
                            <div className="alert" style={{ margin: 0 }}>
                              {publishError}
                            </div>
                          )}

                          <button
                            type="button"
                            className="btn-primary"
                            style={{ fontSize: '12px', padding: '4px 12px' }}
                            disabled={isPublishing || !!slugValidationError}
                            onClick={() => handlePublish(selectedFont.id)}
                            data-testid="font-publish-button"
                          >
                            {isPublishing ? t('font.publishing') : t('font.publishBtn')}
                          </button>
                        </div>
                      </div>
                    </div>
                  ) : (
                    <>
                      <div className="detail-group">
                        <div className="detail-label">{t('font.publishStatus')}</div>
                        <div className="detail-value">
                          <span style={{ color: '#4caf50' }} data-testid="font-published-status">
                            {t('font.published')}
                          </span>
                        </div>
                      </div>

                      <div className="detail-group">
                        <div className="detail-label">{t('font.publicUrl')}</div>
                        <div className="detail-value">
                          <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
                            <div className="form-value code" style={{ fontSize: '12px' }}>
                              /fonts/{selectedFont.slug}/glyphs/{'{fontstack}'}/{'{range}'}.pbf
                            </div>
                            <button
                              type="button"
                              className="btn-text"
                              style={{ fontSize: '12px', padding: 0, textAlign: 'left' }}
                              onClick={() => copyPublicUrl(selectedFont.slug)}
                              data-testid="font-copy-url-button"
                            >
                              {copySuccess ? t('common.copied') : t('font.copyUrl')}
                            </button>
                          </div>
                        </div>
                      </div>

                      <div className="detail-group">
                        <button
                          type="button"
                          className="btn-secondary"
                          style={{ fontSize: '12px', padding: '4px 12px' }}
                          onClick={() => handleUnpublish(selectedFont.id)}
                          data-testid="font-unpublish-button"
                        >
                          {t('font.unpublishBtn')}
                        </button>
                      </div>
                    </>
                  )}
                </>
              )}

              <div
                style={{ marginTop: 'auto', paddingTop: '16px', borderTop: '1px solid #e0e0e0' }}
              >
                <button
                  type="button"
                  className="btn-secondary"
                  style={{ fontSize: '12px', padding: '4px 12px', color: '#d32f2f' }}
                  onClick={() => handleDelete(selectedFont.id)}
                  data-testid="font-delete-button"
                >
                  {t('font.deleteBtn')}
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
