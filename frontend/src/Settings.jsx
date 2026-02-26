import React, { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuth } from './AuthContext.jsx';
import { getSettings, updateSettings } from './api.js';

export default function Settings() {
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
          setError(err.message || '加载设置失败');
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
  }, [user, navigate]);

  async function handleSubmit(e) {
    e.preventDefault();
    setError('');
    setSuccess('');

    const value = parseFloat(maxSizeMb);
    if (isNaN(value) || value < 1) {
      setError('请输入有效的数值（最小 1 MB）');
      return;
    }

    setIsSaving(true);
    try {
      const data = await updateSettings(value);
      setMaxSizeMb(String(data.maxSizeMb));
      setOriginalValue(data.maxSizeMb);
      setSuccess('设置已保存');
    } catch (err) {
      setError(err.message || '保存失败');
    } finally {
      setIsSaving(false);
    }
  }

  function handleReset() {
    setMaxSizeMb(String(originalValue));
    setError('');
    setSuccess('');
  }

  const hasChanges = parseFloat(maxSizeMb) !== originalValue;

  if (!user || user.role !== 'admin') {
    return null;
  }

  return (
    <div className="page">
      <header className="header">
        <div>
          <h1>设置</h1>
          <p className="subtitle">系统配置</p>
        </div>
        <button type="button" className="btn-secondary" onClick={() => navigate('/')}>
          返回
        </button>
      </header>

      <section className="panel" style={{ marginTop: '28px' }}>
        <div className="panel-header">
          <h2>上传设置</h2>
        </div>
        <div className="panel-body" style={{ flexDirection: 'column' }}>
          {isLoading ? (
            <div className="empty">加载中...</div>
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
                <div className="detail-label">最大上传大小 (MB)</div>
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
                    最小值 1 MB
                  </small>
                </div>
              </div>

              <div style={{ display: 'flex', gap: '8px' }}>
                <button type="submit" className="btn-primary" disabled={isSaving || !hasChanges}>
                  {isSaving ? '保存中...' : '保存'}
                </button>
                <button
                  type="button"
                  className="btn-secondary"
                  onClick={handleReset}
                  disabled={isSaving || !hasChanges}
                >
                  重置
                </button>
              </div>
            </form>
          )}
        </div>
      </section>
    </div>
  );
}
