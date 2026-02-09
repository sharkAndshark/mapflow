import React, { useState, useEffect } from 'react';
import { useNavigate, Navigate } from 'react-router-dom';
import { useAuth } from './AuthContext.jsx';
import * as authApi from './auth.js';

export default function Init() {
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
          <div className="loading">加载中...</div>
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
      setError('两次输入的密码不一致');
      return;
    }

    setIsLoading(true);

    try {
      await authApi.initSystem(username, password);
      navigate('/login');
    } catch (err) {
      setError(err.message || '初始化失败');
    } finally {
      setIsLoading(false);
    }
  }

  return (
    <div className="login-page">
      <div className="login-container">
        <div className="login-header">
          <h1>MapFlow</h1>
          <p>首次使用 - 创建管理员账户</p>
        </div>

        <form onSubmit={handleSubmit} className="login-form">
          {error && <div className="alert">{error}</div>}

          <div className="form-group">
            <label htmlFor="username">管理员用户名</label>
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
            <label htmlFor="password">密码</label>
            <input
              id="password"
              type="password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              disabled={isLoading}
              required
              autoComplete="new-password"
            />
            <small>密码必须至少8个字符，包含大小写字母、数字和特殊字符</small>
          </div>

          <div className="form-group">
            <label htmlFor="confirmPassword">确认密码</label>
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
            {isLoading ? '创建中...' : '创建管理员账户'}
          </button>
        </form>
      </div>
    </div>
  );
}
