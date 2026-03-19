import React, { useState, useEffect } from 'react';
import { useNavigate, Navigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useAuth } from './AuthContext.jsx';
import { isInitialized } from './auth.js';
import LanguageSwitcher from './LanguageSwitcher.jsx';

export default function Login() {
  const { t } = useTranslation();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [initChecked, setInitChecked] = useState(false);
  const [isSystemInitialized, setIsSystemInitialized] = useState(true);
  const { login, isAuthenticated, isLoading: authLoading } = useAuth();
  const navigate = useNavigate();

  useEffect(() => {
    async function checkInit() {
      try {
        const initialized = await isInitialized();
        setIsSystemInitialized(initialized ?? false);
        setInitChecked(true);
      } catch {
        setIsSystemInitialized(false);
        setInitChecked(true);
      }
    }
    checkInit();
  }, []);

  if (authLoading || !initChecked) {
    return (
      <div className="login-page">
        <div className="login-container">
          <div className="loading">{t('common.loading')}</div>
        </div>
      </div>
    );
  }

  if (isAuthenticated) {
    return <Navigate to="/" replace />;
  }

  if (!isSystemInitialized) {
    return <Navigate to="/init" replace />;
  }

  async function handleSubmit(e) {
    e.preventDefault();
    setError('');
    setIsLoading(true);

    try {
      await login(username, password);
      navigate('/');
    } catch (err) {
      setError(err.message || t('auth.loginFailed'));
    } finally {
      setIsLoading(false);
    }
  }

  return (
    <div className="login-page">
      <div style={{ position: 'fixed', top: '16px', right: '16px', zIndex: 10 }}>
        <LanguageSwitcher />
      </div>
      <div className="login-container">
        <div className="login-header">
          <h1>MapFlow</h1>
          <p>{t('auth.pleaseLogin')}</p>
        </div>

        <form onSubmit={handleSubmit} className="login-form">
          {error && <div className="alert">{error}</div>}

          <div className="form-group">
            <label htmlFor="username">{t('auth.username')}</label>
            <input
              id="username"
              type="text"
              value={username}
              onChange={(e) => setUsername(e.target.value)}
              disabled={isLoading}
              required
              autoComplete="username"
            />
          </div>

          <div className="form-group">
            <label htmlFor="password">{t('auth.password')}</label>
            <input
              id="password"
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              disabled={isLoading}
              required
              autoComplete="current-password"
            />
          </div>

          <button type="submit" className="btn-primary" disabled={isLoading}>
            {isLoading ? t('auth.logining') : t('auth.login')}
          </button>
        </form>
      </div>
    </div>
  );
}
