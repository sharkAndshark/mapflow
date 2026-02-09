const API_BASE = '/api/auth';

export async function login(username, password) {
  const res = await fetch(`${API_BASE}/login`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    credentials: 'include',
    body: JSON.stringify({ username, password }),
  });

  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || 'Login failed');
  }

  return res.json();
}

export async function logout() {
  const res = await fetch(`${API_BASE}/logout`, {
    method: 'POST',
    credentials: 'include',
  });

  if (!res.ok) {
    throw new Error('Logout failed');
  }

  return res.ok;
}

export async function checkAuth() {
  const res = await fetch(`${API_BASE}/check`, {
    credentials: 'include',
  });

  if (res.status === 401) {
    return null;
  }

  if (!res.ok) {
    throw new Error('Auth check failed');
  }

  return res.json();
}

export async function initSystem(username, password) {
  const res = await fetch(`${API_BASE}/init`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    credentials: 'include',
    body: JSON.stringify({ username, password }),
  });

  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || 'Initialization failed');
  }

  return res.json();
}

export async function isInitialized() {
  const res = await fetch('/api/test/is-initialized', {
    credentials: 'include',
  });

  if (!res.ok) {
    throw new Error(`Failed to check initialization: ${res.status}`);
  }

  const data = await res.json();
  return data.initialized === true;
}
