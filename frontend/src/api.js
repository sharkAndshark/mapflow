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

export async function publishFile(fileId, options = {}) {
  const body = {};
  if (options.slug) body.slug = options.slug;
  if (options.minZoom !== undefined) body.minZoom = options.minZoom;
  if (options.maxZoom !== undefined) body.maxZoom = options.maxZoom;
  if (options.useAliases !== undefined) body.useAliases = options.useAliases;

  const res = await fetchWithAuth(`/api/files/${fileId}/publish`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
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

export async function updateTileZoom(fileId, minZoom, maxZoom) {
  const body = {};
  if (minZoom !== undefined) body.minZoom = minZoom;
  if (maxZoom !== undefined) body.maxZoom = maxZoom;

  const res = await fetchWithAuth(`/api/files/${fileId}/zoom`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '更新缩放层级失败');
  }
  return res.json();
}

export async function updateFieldAliases(fileId, fields) {
  const res = await fetchWithAuth(`/api/files/${fileId}/field-aliases`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ fields }),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '更新字段别名失败');
  }
  return res.json();
}

export async function updatePublishSettings(fileId, settings) {
  const body = {};
  if (settings.useAliases !== undefined) body.useAliases = settings.useAliases;

  const res = await fetchWithAuth(`/api/files/${fileId}/publish-settings`, {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '更新发布设置失败');
  }
  return res.json();
}

export async function getSettings() {
  const res = await fetchWithAuth('/api/settings');
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '获取设置失败');
  }
  return res.json();
}

export async function updateSettings(maxSizeMb) {
  const res = await fetchWithAuth('/api/settings', {
    method: 'PATCH',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ maxSizeMb }),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '更新设置失败');
  }
  return res.json();
}

export async function testPostgisConnection(payload) {
  const res = await fetchWithAuth('/api/postgis/connections/test', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || 'PostGIS 连接测试失败');
  }
  return res.json();
}

export async function registerPostgisSource(payload) {
  const res = await fetchWithAuth('/api/postgis/sources/register', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || 'PostGIS 数据源注册失败');
  }
  return res.json();
}

export async function listWorkspaces() {
  const res = await fetchWithAuth('/api/workspaces');
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '获取工作空间列表失败');
  }
  return res.json();
}

export async function createWorkspace(name) {
  const res = await fetchWithAuth('/api/workspaces', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name }),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '创建工作空间失败');
  }
  return res.json();
}

export async function updateWorkspace(workspaceId, name) {
  const res = await fetchWithAuth(`/api/workspaces/${workspaceId}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ name }),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '更新工作空间失败');
  }
  return res.json();
}

export async function deleteWorkspace(workspaceId) {
  const res = await fetchWithAuth(`/api/workspaces/${workspaceId}`, {
    method: 'DELETE',
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '删除工作空间失败');
  }
  return null;
}

export async function restoreWorkspace(workspaceId, newName) {
  const body = {};
  if (typeof newName === 'string') {
    body.name = newName;
  }

  const res = await fetchWithAuth(`/api/workspaces/${workspaceId}/restore`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '恢复工作空间失败');
  }
  return res.json();
}

export async function listArchivedWorkspaces() {
  const res = await fetchWithAuth('/api/workspaces/archived');
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '获取已归档工作空间失败');
  }
  return res.json();
}

export async function switchWorkspace(workspaceId) {
  const res = await fetchWithAuth('/api/workspaces/current', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ workspaceId }),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '切换工作空间失败');
  }
  return res.json();
}

export async function getCurrentWorkspace() {
  const res = await fetchWithAuth('/api/workspaces/current');
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '获取当前工作空间失败');
  }
  return res.json();
}

export async function listWorkspaceMembers(workspaceId) {
  const res = await fetchWithAuth(`/api/workspaces/${workspaceId}/members`);
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '获取成员列表失败');
  }
  return res.json();
}

export async function inviteWorkspaceMember(workspaceId, username) {
  const res = await fetchWithAuth(`/api/workspaces/${workspaceId}/members`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ username }),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '邀请成员失败');
  }
  return res.json();
}

export async function removeWorkspaceMember(workspaceId, userId) {
  const res = await fetchWithAuth(`/api/workspaces/${workspaceId}/members/${userId}`, {
    method: 'DELETE',
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '移除成员失败');
  }
  return null;
}

export async function leaveWorkspace(workspaceId) {
  const res = await fetchWithAuth(`/api/workspaces/${workspaceId}/leave`, {
    method: 'POST',
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || '离开工作空间失败');
  }
  return null;
}
