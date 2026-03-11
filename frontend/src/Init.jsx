import React, { useState, useEffect } from 'react';
import { useNavigate, Navigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useAuth } from './AuthContext.jsx';
import * as authApi from './auth.js';

export default function Init() {
  const { t } = useTranslation();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [error, setError] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [isCheckingInit, setIsCheckingInit] = useState(true);
  const [isInitialized, setIsInitialized] = useState(false);
  const { isAuthenticated, isLoading: authLoading } = useAuth();
  const navigate = useNavigate();

  useEffect(() => {
    async function checkInit() {
      try {
        const initialized = await authApi.isInitialized();
        setIsInitialized(initialized);
        setIsCheckingInit(false);

        if (initialized) {
          navigate('/login');
        }
      } catch (err) {
        console.error('Failed to check initialization status:', err);
        setIsCheckingInit(false);
      }
    }

    checkInit();
  }, [navigate]);

  if (authLoading || isCheckingInit) {
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

  if (isInitialized) {
    return <Navigate to="/login" replace />;
  }

  async function handleSubmit(e) {
    e.preventDefault();
    setError('');

    if (password !== confirmPassword) {
      setError(t('auth.passwordMismatch'));
      return;
    }

    setIsLoading(true);

    try {
      await authApi.initSystem(username, password);
      navigate('/login');
    } catch (err) {
      setError(err.message || t('auth.initFailed'));
    } finally {
      setIsLoading(false);
    }
  }

  return (
    <div className="login-page">
      <div className="login-container">
        <div className="login-header">
          <h1>MapFlow</h1>
          <p>{t('auth.setupDesc')}</p>
        </div>

        <form onSubmit={handleSubmit} className="login-form">
          {error && (
            <div className="alert" data-testid="error-alert">
              {error}
            </div>
          )}

          <div className="form-group">
            <label htmlFor="username">{t('auth.adminUsername')}</label>
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
              autoComplete="new-password"
            />
            <small>{t('auth.passwordHint')}</small>
          </div>

          <div className="form-group">
            <label htmlFor="confirmPassword">{t('auth.confirmPassword')}</label>
            <input
              id="confirmPassword"
              type="password"
              value={confirmPassword}
              onChange={(e) => setConfirmPassword(e.target.value)}
              disabled={isLoading}
              required
              autoComplete="new-password"
            />
          </div>

          <button type="submit" className="btn-primary" disabled={isLoading}>
            {isLoading ? t('auth.creating') : t('auth.createAdmin')}
          </button>
        </form>
      </div>
    </div>
  );
}
