import React, { createContext, useContext, useState, useEffect } from 'react';
import * as authApi from './auth.js';

const AuthContext = createContext(null);

export function AuthProvider({ children }) {
  const [user, setUser] = useState(null);
  const [isLoading, setIsLoading] = useState(true);
  const [authError, setAuthError] = useState(null);

  useEffect(() => {
    let cancelled = false;

    async function checkAuth() {
      try {
        const user = await authApi.checkAuth();
        // checkAuth returns null for 401 (unauthorized)
        // and throws for other errors (network, 500, etc.)
        if (!cancelled) {
          setUser(user);
          setAuthError(null);
        }
      } catch (error) {
        if (!cancelled) {
          // Only set user to null if it's a 401
          // For other errors (network, server), keep previous state and set error
          console.error('Auth check failed:', error);
          if (error.message && error.message.includes('401')) {
            setUser(null);
            setAuthError(null);
          } else {
            // Network error or server error - don't clear user, but set error state
            // This allows the app to continue with cached user data while showing error
            setAuthError(error.message || 'Auth check failed');
          }
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
    authError,
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
