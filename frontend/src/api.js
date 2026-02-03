const API_BASE = import.meta.env.DEV ? 'http://localhost:3000' : '';

const parseResponse = async (response, fallbackMessage) => {
  if (response.ok) return response.json();
  let message = fallbackMessage;
  try {
    const data = await response.json();
    message = data?.error?.message || data?.message || fallbackMessage;
  } catch (_) {
    // ignore parsing errors
  }
  throw new Error(message);
};

export const api = {
  // Get configuration
  getConfig: async () => {
    const response = await fetch(`${API_BASE}/config`);
    return parseResponse(response, 'Failed to fetch config');
  },

  // Verify configuration
  verifyConfig: async (config) => {
    const response = await fetch(`${API_BASE}/verify`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(config)
    });
    return parseResponse(response, 'Failed to verify config');
  },

  // Apply configuration
  applyConfig: async (config) => {
    const response = await fetch(`${API_BASE}/config`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(config)
    });
    return parseResponse(response, 'Failed to apply config');
  },

  // Upload shapefile
  uploadFile: async (file, srid = null) => {
    const formData = new FormData();
    formData.append('file', file);
    if (srid) formData.append('srid', srid);

    const response = await fetch(`${API_BASE}/upload`, {
      method: 'POST',
      body: formData
    });
    return parseResponse(response, 'Failed to upload file');
  },

  // Get resource metadata (fields, bounds)
  getResourceMetadata: async (resourceId) => {
    const response = await fetch(`${API_BASE}/resources/${resourceId}/metadata`);
    return parseResponse(response, 'Failed to fetch resource metadata');
  }
};
