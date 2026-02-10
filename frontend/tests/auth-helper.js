// Authentication helper for E2E tests
export async function setupTestUser(request) {
  // Initialize system if not already initialized
  try {
    await request.post('/api/auth/init', {
      data: {
        username: 'admin',
        password: 'Test123!@#',
      },
    });
  } catch (e) {
    // Ignore if already initialized (409 Conflict)
    // Playwright throws an error with response status in message or as a property
    const status = e.response?.status?.() || e.response?.status || e.status;
    if (status !== 409 && !e.message?.includes('409')) {
      throw e;
    }
  }};

export async function loginUser(request, username = 'admin', password = 'Test123!@#') {
  await request.post('/api/auth/login', {
    data: {
      username,
      password,
    },
  });
}

export async function logoutUser(request) {
  await request.post('/api/auth/logout');
}
