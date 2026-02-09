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
