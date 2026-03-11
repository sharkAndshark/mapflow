import React, { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useAuth } from './AuthContext.jsx';
import { getSettings, updateSettings } from './api.js';

export default function Settings() {
  const { t } = useTranslation();
  const { user } = useAuth();
  const navigate = useNavigate();
  const [maxSizeMb, setMaxSizeMb] = useState('');
  const [originalValue, setOriginalValue] = useState(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState('');
  const [success, setSuccess] = useState('');

  useEffect(() => {
    if (!user) return;
    if (user.role !== 'admin') {
      navigate('/');
      return;
    }

    let cancelled = false;
    async function load() {
      try {
        const data = await getSettings();
        if (!cancelled) {
          setMaxSizeMb(String(data.maxSizeMb));
          setOriginalValue(data.maxSizeMb);
        }
      } catch (err) {
        if (!cancelled) {
          setError(err.message || t('settings.loadFailed'));
        }
      } finally {
        if (!cancelled) {
          setIsLoading(false);
        }
      }
    }
    load();
    return () => {
      cancelled = true;
    };
  }, [user, navigate, t]);

  async function handleSubmit(e) {
    e.preventDefault();
    setError('');
    setSuccess('');

    const value = parseInt(maxSizeMb, 10);
    if (isNaN(value) || value < 1) {
      setError(t('settings.invalidValue'));
      return;
    }
    if (value > 102400) {
      setError(t('settings.maxValueExceeded'));
      return;
    }

    setIsSaving(true);
    try {
      const data = await updateSettings(value);
      setMaxSizeMb(String(data.maxSizeMb));
      setOriginalValue(data.maxSizeMb);
      setSuccess(t('settings.saved'));
    } catch (err) {
      setError(err.message || t('settings.saveFailed'));
    } finally {
      setIsSaving(false);
    }
  }

  function handleReset() {
    setMaxSizeMb(String(originalValue));
    setError('');
    setSuccess('');
  }

  const hasChanges = parseInt(maxSizeMb, 10) !== originalValue;
  const isValid =
    !isNaN(parseInt(maxSizeMb, 10)) &&
    parseInt(maxSizeMb, 10) >= 1 &&
    parseInt(maxSizeMb, 10) <= 102400;

  if (!user || user.role !== 'admin') {
    return null;
  }

  return (
    <div className="page">
      <header className="header">
        <div>
          <h1>{t('settings.title')}</h1>
          <p className="subtitle">{t('settings.subtitle')}</p>
        </div>
        <button type="button" className="btn-secondary" onClick={() => navigate('/')}>
          {t('common.back')}
        </button>
      </header>

      <section className="panel" style={{ marginTop: '28px' }}>
        <div className="panel-header">
          <h2>{t('settings.uploadSettings')}</h2>
        </div>
        <div className="panel-body" style={{ flexDirection: 'column' }}>
          {isLoading ? (
            <div className="empty">{t('common.loading')}</div>
          ) : (
            <form
              onSubmit={handleSubmit}
              style={{ padding: '24px', display: 'flex', flexDirection: 'column', gap: '20px' }}
            >
              {error && <div className="alert">{error}</div>}
              {success && (
                <div
                  style={{
                    padding: '12px 16px',
                    borderRadius: '10px',
                    background: '#f0fff0',
                    color: '#1a7a1a',
                    border: '1px solid #caf0ca',
                  }}
                >
                  {success}
                </div>
              )}

              <div className="detail-group">
                <div className="detail-label">{t('settings.maxUploadSize')}</div>
                <div className="detail-value">
                  <input
                    type="number"
                    step="1"
                    min="1"
                    value={maxSizeMb}
                    onChange={(e) => setMaxSizeMb(e.target.value)}
                    className="form-input"
                    style={{ width: '200px' }}
                    disabled={isSaving}
                  />
                  <small className="form-hint" style={{ display: 'block', marginTop: '4px' }}>
                    {t('settings.maxSizeHint')}
                  </small>
                </div>
              </div>

              <div style={{ display: 'flex', gap: '8px' }}>
                <button
                  type="submit"
                  className="btn-primary"
                  disabled={isSaving || !hasChanges || !isValid}
                >
                  {isSaving ? t('common.saving') : t('common.save')}
                </button>
                <button
                  type="button"
                  className="btn-secondary"
                  onClick={handleReset}
                  disabled={isSaving || !hasChanges}
                >
                  {t('common.reset')}
                </button>
              </div>
            </form>
          )}
        </div>
      </section>
    </div>
  );
}
