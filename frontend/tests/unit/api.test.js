import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { fetchWithAuth, setAuthContext } from '../../src/api.js';

describe('fetchWithAuth', () => {
  beforeEach(() => {
    global.fetch = vi.fn();
    global.window = { location: { href: '/' } };
  });

  afterEach(() => {
    setAuthContext(null);
    vi.restoreAllMocks();
    delete global.fetch;
    delete global.window;
  });

  it('logs out and redirects on unauthorized responses when auth context is registered', async () => {
    const logout = vi.fn().mockResolvedValue(undefined);
    setAuthContext({ logout });
    global.fetch.mockResolvedValue({ status: 401 });

    await expect(fetchWithAuth('/api/workspaces')).rejects.toThrow('Unauthorized');

    expect(logout).toHaveBeenCalledTimes(1);
    expect(global.fetch).toHaveBeenCalledWith('/api/workspaces', {
      credentials: 'include',
    });
    expect(global.window.location.href).toBe('/login');
  });

  it('still throws unauthorized without redirect state when no auth context is registered', async () => {
    global.fetch.mockResolvedValue({ status: 401 });

    await expect(fetchWithAuth('/api/settings')).rejects.toThrow('Unauthorized');

    expect(global.window.location.href).toBe('/');
  });
});
