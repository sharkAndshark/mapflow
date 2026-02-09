import React, { createContext, useContext, useState, useEffect } from 'react';
import * as authApi from './auth.js';

const AuthContext = createContext(null);

export function AuthProvider({ children }) {
  const [user, setUser] = useState(null);
  const [isLoading, setIsLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;

    async function checkAuth() {
      try {
        const user = await authApi.checkAuth();
        if (!cancelled) {
          setUser(user);
        }
      } catch (error) {
        if (!cancelled) {
          console.error('Auth check failed:', error);
          setUser(null);
        }
      } finally {
        if (!cancelled) {
          setIsLoading(false);
        }
      }
    }

    checkAuth();

    return () => {
      cancelled = true;
    };
  }, []);

  const login = async (username, password) => {
    const user = await authApi.login(username, password);
    setUser(user);
    return user;
  };

  const logout = async () => {
    await authApi.logout();
    setUser(null);
  };

  const initSystem = async (username, password) => {
    return await authApi.initSystem(username, password);
  };

  const value = {
    user,
    isLoading,
    login,
    logout,
    initSystem,
    isAuthenticated: !!user,
  };

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth() {
  const context = useContext(AuthContext);
  if (!context) {
    throw new Error('useAuth must be used within an AuthProvider');
  }
  return context;
}
