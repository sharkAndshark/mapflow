import React from 'react';
import { createRoot } from 'react-dom/client';
import { BrowserRouter, Routes, Route } from 'react-router-dom';
import { AuthProvider } from './AuthContext.jsx';
import ProtectedRoute from './ProtectedRoute.jsx';
import App from './App.jsx';
import Login from './Login.jsx';
import Init from './Init.jsx';
import Preview from './Preview.jsx';
import PublicMap from './PublicMap.jsx';
import './index.css';

const root = createRoot(document.getElementById('root'));
root.render(
  <BrowserRouter>
    <AuthProvider>
      <Routes>
        <Route path="/login" element={<Login />} />
        <Route path="/init" element={<Init />} />
        <Route
          path="/"
          element={
            <ProtectedRoute>
              <App />
            </ProtectedRoute>
          }
        />
        <Route
          path="/preview/:id"
          element={
            <ProtectedRoute>
              <Preview />
            </ProtectedRoute>
          }
        />
        <Route path="/tiles/:slug" element={<PublicMap />} />
      </Routes>
    </AuthProvider>
  </BrowserRouter>,
);
