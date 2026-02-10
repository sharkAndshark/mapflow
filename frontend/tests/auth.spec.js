import { test, expect } from './fixtures';

test.describe('Authentication Flow', () => {
  test.beforeEach(async ({ workerServer }) => {
    await workerServer.reset();
  });

  test('system initialization flow', async ({ page }) => {
    // First visit should redirect to init page
    await page.goto('/');
    await expect(page).toHaveURL(/\/init/);

    // Check init page elements
    await expect(page.locator('h1')).toContainText('MapFlow');
    await expect(page.locator('.login-header p')).toContainText('首次使用');

    // Fill init form
    await page.fill('#username', 'admin');
    await page.fill('#password', 'Test123!@#');
    await page.fill('#confirmPassword', 'Test123!@#');

    // Submit form and wait for navigation
    await Promise.all([page.waitForURL(/\/login/), page.click('button[type="submit"]')]);

    // Should redirect to login page
    await expect(page).toHaveURL(/\/login/);
    await expect(page.locator('h1')).toContainText('MapFlow');
    await expect(page.locator('.login-header p')).toContainText('请登录以继续');
  });

  test('login with correct credentials', async ({ page, request }) => {
    // First initialize the system
    await request.post('/api/auth/init', {
      data: {
        username: 'admin',
        password: 'Test123!@#',
      },
    });

    // Visit login page
    await page.goto('/login');
    await expect(page.locator('h1')).toContainText('MapFlow');

    // Fill login form
    await page.fill('#username', 'admin');
    await page.fill('#password', 'Test123!@#');

    // Submit form
    await page.click('button[type="submit"]');

    // Should redirect to home page
    await expect(page).toHaveURL(/\/$/);
    await expect(page.locator('.header')).toContainText('admin');
    await expect(page.locator('.header')).toContainText('(admin)');
  });

  test('login with incorrect credentials shows error', async ({ page, request }) => {
    // First initialize the system
    await request.post('/api/auth/init', {
      data: {
        username: 'admin',
        password: 'Test123!@#',
      },
    });

    // Visit login page
    await page.goto('/login');

    // Fill login form with wrong password
    await page.fill('#username', 'admin');
    await page.fill('#password', 'WrongPassword123!');

    // Submit form
    await page.click('button[type="submit"]');

    // Should show error
    await expect(page.locator('.alert')).toBeVisible();
    await expect(page.locator('.alert')).toContainText('Invalid credentials');
    await expect(page).toHaveURL(/\/login/);
  });

  test('logout redirects to login page', async ({ page, request }) => {
    // First initialize and login
    await request.post('/api/auth/init', {
      data: {
        username: 'admin',
        password: 'Test123!@#',
      },
    });

    // Login via API
    await request.post('/api/auth/login', {
      data: {
        username: 'admin',
        password: 'Test123!@#',
      },
    });

    // Visit home page
    await page.goto('/');
    await expect(page.locator('.header')).toContainText('admin');

    // Click logout button
    await page.click('button:has-text("登出")');

    // Should redirect to login
    await expect(page).toHaveURL(/\/login/);
  });

  test('protected routes redirect to login when not authenticated', async ({ page, request }) => {
    // First initialize the system
    await request.post('/api/auth/init', {
      data: {
        username: 'admin',
        password: 'Test123!@#',
      },
    });

    // Try to visit home page without auth
    await page.goto('/');
    await expect(page).toHaveURL(/\/login/);

    // Try to visit preview page without auth
    await page.goto('/preview/some-id');
    await expect(page).toHaveURL(/\/login/);
  });

  test('password validation on init page', async ({ page }) => {
    await page.goto('/init');

    // Try too short password
    await page.fill('#username', 'admin');
    await page.fill('#password', 'short');
    await page.fill('#confirmPassword', 'short');
    await page.click('button[type="submit"]');

    // Should show validation error (from backend)
    await expect(page.locator('.alert')).toBeVisible();
  });

  test('password mismatch on init page', async ({ page }) => {
    await page.goto('/init');

    await page.fill('#username', 'admin');
    await page.fill('#password', 'Test123!@#');
    await page.fill('#confirmPassword', 'Different123!@#');
    await page.click('button[type="submit"]');

    // Should show mismatch error
    await expect(page.locator('.alert')).toContainText('两次输入的密码不一致');
  });

  test('cannot initialize system twice', async ({ page, request }) => {
    // Initialize system
    await request.post('/api/auth/init', {
      data: {
        username: 'admin',
        password: 'Test123!@#',
      },
    });

    // Try to visit init page again
    await page.goto('/init');

    // Should redirect to login
    await expect(page).toHaveURL(/\/login/);

    // Try to call init API again
    const response = await request.post('/api/auth/init', {
      data: {
        username: 'admin2',
        password: 'Test123!@#',
      },
    });

    expect(response.status()).toBe(409);
    const data = await response.json();
    expect(data.error).toContain('already initialized');
  });

  test('check authentication status', async ({ request }) => {
    // Before login
    let response = await request.get('/api/auth/check');
    expect(response.status()).toBe(401);

    // Initialize
    await request.post('/api/auth/init', {
      data: {
        username: 'admin',
        password: 'Test123!@#',
      },
    });

    // After init but before login
    response = await request.get('/api/auth/check');
    expect(response.status()).toBe(401);

    // After login
    await request.post('/api/auth/login', {
      data: {
        username: 'admin',
        password: 'Test123!@#',
      },
    });

    response = await request.get('/api/auth/check');
    expect(response.status()).toBe(200);
    const data = await response.json();
    expect(data.username).toBe('admin');
    expect(data.role).toBe('admin');
  });
});
