import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig(() => {
  const backendPort = process.env.PORT || '3000';
  const frontendPort = process.env.VITE_PORT || '5173';

  return {
    plugins: [react()],
    server: {
      port: Number(frontendPort),
      proxy: {
        '/api': `http://localhost:${backendPort}`
      }
    }
  };
});
