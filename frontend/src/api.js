import { useAuth } from './AuthContext.jsx';

let authContext = null;

export function setAuthContext(context) {
  authContext = context;
}

export async function fetchWithAuth(url, options = {}) {
  const modifiedOptions = {
    ...options,
    credentials: 'include',
  };

  const response = await fetch(url, modifiedOptions);

  if (response.status === 401) {
    if (authContext) {
      authContext.logout();
      window.location.href = '/login';
    }
    throw new Error('Unauthorized');
  }

  return response;
}

export async function publishFile(fileId, slug) {
  const res = await fetchWithAuth(`/api/files/${fileId}/publish`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(slug ? { slug } : {}),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '发布失败');
  }
  return res.json();
}

export async function unpublishFile(fileId) {
  const res = await fetchWithAuth(`/api/files/${fileId}/unpublish`, {
    method: 'POST',
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '取消发布失败');
  }
  return res.json();
}
