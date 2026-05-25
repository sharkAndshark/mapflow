import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { listMaps, createMap, deleteMap } from './api.js';
import { formatSize } from './utils.js';

export default function MapsPanel() {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const [maps, setMaps] = useState([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState('');
  const [showCreate, setShowCreate] = useState(false);
  const [newName, setNewName] = useState('');
  const [isCreating, setIsCreating] = useState(false);

  const dateTimeFormatter = useMemo(
    () =>
      new Intl.DateTimeFormat(i18n.resolvedLanguage || i18n.language || undefined, {
        dateStyle: 'medium',
        timeStyle: 'short',
      }),
    [i18n.language, i18n.resolvedLanguage],
  );

  useEffect(() => {
    let cancelled = false;
    async function load() {
      try {
        const data = await listMaps();
        if (!cancelled) {
          setMaps(Array.isArray(data) ? data : []);
          setError('');
        }
      } catch (err) {
        if (!cancelled) {
          setError(err.message || t('map.loadFailed'));
        }
      } finally {
        if (!cancelled) setIsLoading(false);
      }
    }
    load();
    return () => {
      cancelled = true;
    };
  }, [t]);

  async function handleCreate(e) {
    e.preventDefault();
    const name = newName.trim();
    if (!name) return;
    setIsCreating(true);
    try {
      const map = await createMap(name);
      setMaps((prev) => [map, ...prev]);
      setNewName('');
      setShowCreate(false);
      navigate(`/editor/${map.id}`);
    } catch (err) {
      setError(err.message || t('map.createFailed'));
    } finally {
      setIsCreating(false);
    }
  }

  async function handleDelete(mapId, mapName) {
    if (!confirm(t('map.deleteConfirm', { name: mapName }))) return;
    try {
      await deleteMap(mapId);
      setMaps((prev) => prev.filter((m) => m.id !== mapId));
    } catch (err) {
      setError(err.message || t('map.deleteFailed'));
    }
  }

  if (isLoading) {
    return <div className="empty">{t('common.loading')}</div>;
  }

  return (
    <div role="tabpanel" id="main-tabpanel-maps">
      <div style={{ padding: '12px 16px', borderBottom: '1px solid #e0e0e0' }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          <span style={{ fontSize: '13px', color: '#666' }}>
            {maps.length} {maps.length === 1 ? t('map.mapCountSingle') : t('map.mapCountPlural')}
          </span>
          <button
            type="button"
            className="btn-primary"
            style={{ fontSize: '12px', padding: '4px 12px' }}
            onClick={() => setShowCreate(true)}
          >
            {t('map.createMap')}
          </button>
        </div>
      </div>

      {error && (
        <div className="alert" style={{ margin: '8px 16px' }}>
          {error}
        </div>
      )}

      {showCreate && (
        <form
          onSubmit={handleCreate}
          style={{
            padding: '12px 16px',
            borderBottom: '1px solid #e0e0e0',
            background: '#f8f9fa',
          }}
        >
          <div style={{ display: 'flex', gap: '8px' }}>
            <input
              type="text"
              className="form-input"
              style={{ flex: 1, fontSize: '13px' }}
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              placeholder={t('map.namePlaceholder')}
              autoFocus
              disabled={isCreating}
            />
            <button
              type="submit"
              className="btn-primary"
              style={{ fontSize: '12px', padding: '4px 12px' }}
              disabled={isCreating || !newName.trim()}
            >
              {isCreating ? t('common.saving') : t('common.submit')}
            </button>
            <button
              type="button"
              className="btn-secondary"
              style={{ fontSize: '12px', padding: '4px 12px' }}
              onClick={() => {
                setShowCreate(false);
                setNewName('');
              }}
              disabled={isCreating}
            >
              {t('common.cancel')}
            </button>
          </div>
        </form>
      )}

      {maps.length === 0 ? (
        <div className="empty" style={{ padding: '24px 16px' }}>
          {t('map.noMaps')}
        </div>
      ) : (
        <div style={{ padding: '0 16px' }}>
          {maps.map((map) => (
            <div
              key={map.id}
              style={{
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                padding: '10px 0',
                borderBottom: '1px solid #f0f0f0',
              }}
            >
              <div
                style={{
                  flex: 1,
                  cursor: 'pointer',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: '2px',
                }}
                onClick={() => navigate(`/editor/${map.id}`)}
                role="button"
                tabIndex={0}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') {
                    e.preventDefault();
                    navigate(`/editor/${map.id}`);
                  }
                }}
              >
                <span style={{ fontSize: '14px', fontWeight: 500 }}>{map.name}</span>
                <span style={{ fontSize: '11px', color: '#888' }}>
                  {map.updatedAt ? dateTimeFormatter.format(new Date(map.updatedAt)) : ''}
                  {map.isPublic && (
                    <span style={{ color: '#4caf50', marginLeft: '8px' }}>
                      {t('map.published')}
                    </span>
                  )}
                </span>
              </div>
              <div style={{ display: 'flex', gap: '8px', alignItems: 'center' }}>
                <button
                  type="button"
                  className="btn-text"
                  style={{ fontSize: '12px' }}
                  onClick={(e) => {
                    e.stopPropagation();
                    navigate(`/editor/${map.id}`);
                  }}
                >
                  {t('map.edit')}
                </button>
                <button
                  type="button"
                  className="btn-text"
                  style={{ fontSize: '12px', color: '#d32f2f' }}
                  onClick={(e) => {
                    e.stopPropagation();
                    handleDelete(map.id, map.name);
                  }}
                >
                  {t('common.delete')}
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
