let authContext = null;

function extractBackendError(data) {
  if (data && typeof data.error === 'string') {
    return data.error;
  }
  return '';
}

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
      try {
        await authContext.logout();
      } catch (error) {
        console.error('Failed to logout after unauthorized response:', error);
      }
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
    throw new Error(extractBackendError(data));
  }
  return res.json();
}

export async function listFiles() {
  const res = await fetchWithAuth('/api/files');
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(extractBackendError(data));
  }
  return res.json();
}

export async function unpublishFile(fileId) {
  const res = await fetchWithAuth(`/api/files/${fileId}/unpublish`, {
    method: 'POST',
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(extractBackendError(data));
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
    throw new Error(extractBackendError(data));
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
    throw new Error(extractBackendError(data));
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
    throw new Error(extractBackendError(data));
  }
  return res.json();
}

export async function getSettings() {
  const res = await fetchWithAuth('/api/settings');
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(extractBackendError(data));
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
    throw new Error(extractBackendError(data));
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
    throw new Error(extractBackendError(data));
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
    throw new Error(extractBackendError(data));
  }
  return res.json();
}

export async function listWorkspaces() {
  const res = await fetchWithAuth('/api/workspaces');
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(extractBackendError(data));
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
    throw new Error(extractBackendError(data));
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
    throw new Error(extractBackendError(data));
  }
  return res.json();
}

export async function deleteWorkspace(workspaceId) {
  const res = await fetchWithAuth(`/api/workspaces/${workspaceId}`, {
    method: 'DELETE',
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(extractBackendError(data));
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
    throw new Error(extractBackendError(data));
  }
  return res.json();
}

export async function listArchivedWorkspaces() {
  const res = await fetchWithAuth('/api/workspaces/archived');
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(extractBackendError(data));
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
    throw new Error(extractBackendError(data));
  }
  return res.json();
}

export async function getCurrentWorkspace() {
  const res = await fetchWithAuth('/api/workspaces/current');
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(extractBackendError(data));
  }
  return res.json();
}

export async function listWorkspaceMembers(workspaceId) {
  const res = await fetchWithAuth(`/api/workspaces/${workspaceId}/members`);
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(extractBackendError(data));
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
    throw new Error(extractBackendError(data));
  }
  return res.json();
}

export async function removeWorkspaceMember(workspaceId, userId) {
  const res = await fetchWithAuth(`/api/workspaces/${workspaceId}/members/${userId}`, {
    method: 'DELETE',
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(extractBackendError(data));
  }
  return null;
}

export async function leaveWorkspace(workspaceId) {
  const res = await fetchWithAuth(`/api/workspaces/${workspaceId}/leave`, {
    method: 'POST',
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(extractBackendError(data));
  }
  return null;
}

export async function listFonts() {
  const res = await fetchWithAuth('/api/fonts');
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(extractBackendError(data));
  }
  return res.json();
}

export async function uploadFont(file, onProgress) {
  const formData = new FormData();
  formData.append('file', file);

  const res = await fetchWithAuth('/api/fonts', {
    method: 'POST',
    body: formData,
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(extractBackendError(data));
  }
  return res.json();
}

export async function getFont(fontId) {
  const res = await fetchWithAuth(`/api/fonts/${fontId}`);
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(extractBackendError(data));
  }
  return res.json();
}

export async function deleteFont(fontId) {
  const res = await fetchWithAuth(`/api/fonts/${fontId}`, {
    method: 'DELETE',
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(extractBackendError(data));
  }
  return null;
}

export async function publishFont(fontId, options = {}) {
  const body = {};
  if (options.slug) body.slug = options.slug;

  const res = await fetchWithAuth(`/api/fonts/${fontId}/publish`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(extractBackendError(data));
  }
  return res.json();
}

export async function unpublishFont(fontId) {
  const res = await fetchWithAuth(`/api/fonts/${fontId}/unpublish`, {
    method: 'POST',
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(extractBackendError(data));
  }
  return null;
}
