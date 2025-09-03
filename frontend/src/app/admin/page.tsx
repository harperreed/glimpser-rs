//! ABOUTME: Admin panel for managing users, API keys, templates, and system
//! ABOUTME: Replaces static/js/admin.js with React components

'use client';

import { useState, useEffect, useCallback } from 'react';
import { ProtectedRoute } from '@/components/ProtectedRoute';
import { apiClient } from '@/lib/api';
import { useAuth } from '@/contexts/auth';
import { useRouter } from 'next/navigation';

interface User {
  id: string;
  username: string;
  email: string;
  created_at: string;
  updated_at: string;
}

interface ApiKey {
  id: string;
  name: string;
  key_hash: string;
  created_at: string;
  updated_at: string;
}

interface Template {
  id: string;
  user_id: string;
  name: string;
  description?: string;
  type: string;
  is_default: boolean;
  created_at: string;
  updated_at: string;
}

type ActiveTab = 'users' | 'api-keys' | 'templates' | 'system';

export default function AdminPage() {
  const [activeTab, setActiveTab] = useState<ActiveTab>('users');
  const [users, setUsers] = useState<User[]>([]);
  const [apiKeys, setApiKeys] = useState<ApiKey[]>([]);
  const [templates, setTemplates] = useState<Template[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const { user, logout } = useAuth();
  const router = useRouter();

  const loadUsers = useCallback(async () => {
    try {
      setError(null);
      const data = await apiClient.get('/settings/users');
      setUsers(Array.isArray(data) ? data : []);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load users');
    }
  }, []);

  const loadApiKeys = useCallback(async () => {
    try {
      setError(null);
      const data = await apiClient.get('/settings/api-keys');
      setApiKeys(Array.isArray(data) ? data : []);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load API keys');
    }
  }, []);

  const loadTemplates = useCallback(async () => {
    try {
      setError(null);
      const data = await apiClient.get('/settings/templates');
      setTemplates(Array.isArray(data) ? data : []);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load templates');
    }
  }, []);

  const loadTabData = useCallback(async (tab: ActiveTab) => {
    setLoading(true);
    try {
      switch (tab) {
        case 'users':
          await loadUsers();
          break;
        case 'api-keys':
          await loadApiKeys();
          break;
        case 'templates':
          await loadTemplates();
          break;
        case 'system':
          // System tab doesn't need API calls
          break;
      }
    } finally {
      setLoading(false);
    }
  }, [loadUsers, loadApiKeys, loadTemplates]);

  useEffect(() => {
    loadTabData(activeTab);
  }, [activeTab, loadTabData]);

  const handleLogout = () => {
    logout();
    router.push('/login');
  };

  const formatDate = (dateStr: string) => {
    return new Date(dateStr).toLocaleDateString();
  };

  const deleteUser = async (userId: string) => {
    if (!confirm('Are you sure you want to delete this user?')) return;
    
    try {
      await apiClient.delete(`/settings/users/${userId}`);
      await loadUsers();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete user');
    }
  };

  const deleteApiKey = async (keyId: string) => {
    if (!confirm('Are you sure you want to delete this API key?')) return;
    
    try {
      await apiClient.delete(`/settings/api-keys/${keyId}`);
      await loadApiKeys();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete API key');
    }
  };

  const deleteTemplate = async (templateId: string) => {
    if (!confirm('Are you sure you want to delete this template?')) return;
    
    try {
      await apiClient.delete(`/settings/templates/${templateId}`);
      await loadTemplates();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete template');
    }
  };

  return (
    <ProtectedRoute>
      <div className="min-h-screen bg-slate-50">
        {/* Header */}
        <nav className="bg-white border-b border-gray-300 px-8 py-4 flex justify-between items-center shadow-sm">
          <div>
            <h1 className="text-xl font-bold text-blue-600">🔍 Glimpser</h1>
          </div>
          <div className="flex items-center gap-6">
            <button 
              onClick={() => router.push('/dashboard')}
              className="text-gray-500 font-medium hover:text-blue-600 transition-colors duration-200"
            >
              Dashboard
            </button>
            <button 
              onClick={() => router.push('/streams')}
              className="text-gray-500 font-medium hover:text-blue-600 transition-colors duration-200"
            >
              Streams
            </button>
            <span className="text-blue-600 font-medium">Admin</span>
            <span className="text-sm text-gray-500">
              Welcome, {user?.username || user?.email}
            </span>
            <button 
              onClick={handleLogout}
              className="px-6 py-3 bg-gray-500 text-white rounded-md font-medium hover:bg-gray-600 transition-all duration-200"
            >
              Logout
            </button>
          </div>
        </nav>

        {/* Main Content */}
        <main className="p-8 max-w-6xl mx-auto w-full">
          <div className="flex justify-between items-center mb-8">
            <h2 className="text-2xl font-bold text-gray-800">Admin Panel</h2>
            <div className="bg-yellow-100 text-yellow-800 px-4 py-2 rounded-md text-sm font-medium">
              <span>⚠️ Administrator privileges required</span>
            </div>
          </div>

          {/* Tabs */}
          <div className="flex gap-1 mb-8 border-b border-gray-300">
            {[
              { key: 'users', label: 'Users' },
              { key: 'api-keys', label: 'API Keys' },
              { key: 'templates', label: 'Templates' },
              { key: 'system', label: 'System' }
            ].map((tab) => (
              <button
                key={tab.key}
                onClick={() => setActiveTab(tab.key as ActiveTab)}
                className={`px-6 py-3 font-medium cursor-pointer border-b-2 transition-all duration-200 ${
                  activeTab === tab.key
                    ? 'border-blue-600 text-blue-600'
                    : 'border-transparent hover:text-blue-600'
                }`}
              >
                {tab.label}
              </button>
            ))}
          </div>

          {/* Error Display */}
          {error && (
            <div className="bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-6">
              {error}
            </div>
          )}

          {/* Users Tab */}
          {activeTab === 'users' && (
            <div>
              <div className="flex justify-between items-center mb-6">
                <h3 className="text-lg font-semibold text-gray-800">User Management</h3>
                <button className="inline-flex items-center justify-center gap-2 px-6 py-3 bg-blue-600 text-white rounded-md font-medium hover:bg-blue-700 transition-all duration-200">
                  Create User
                </button>
              </div>

              <div className="bg-white rounded-md shadow-sm overflow-hidden">
                <table className="w-full border-collapse">
                  <thead>
                    <tr>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Username</th>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Email</th>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Created</th>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {loading ? (
                      <tr>
                        <td colSpan={4} className="px-4 py-4 text-center text-gray-500 italic">Loading users...</td>
                      </tr>
                    ) : users.length === 0 ? (
                      <tr>
                        <td colSpan={4} className="px-4 py-4 text-center text-gray-500 italic">No users found</td>
                      </tr>
                    ) : (
                      users.map((user) => (
                        <tr key={user.id} className="hover:bg-gray-50">
                          <td className="px-4 py-4 border-b border-gray-200">{user.username}</td>
                          <td className="px-4 py-4 border-b border-gray-200">{user.email}</td>
                          <td className="px-4 py-4 border-b border-gray-200">{formatDate(user.created_at)}</td>
                          <td className="px-4 py-4 border-b border-gray-200">
                            <button 
                              onClick={() => deleteUser(user.id)}
                              className="text-red-600 hover:text-red-800 font-medium"
                            >
                              Delete
                            </button>
                          </td>
                        </tr>
                      ))
                    )}
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {/* API Keys Tab */}
          {activeTab === 'api-keys' && (
            <div>
              <div className="flex justify-between items-center mb-6">
                <h3 className="text-lg font-semibold text-gray-800">API Key Management</h3>
                <button className="inline-flex items-center justify-center gap-2 px-6 py-3 bg-blue-600 text-white rounded-md font-medium hover:bg-blue-700 transition-all duration-200">
                  Generate API Key
                </button>
              </div>

              <div className="bg-white rounded-md shadow-sm overflow-hidden">
                <table className="w-full border-collapse">
                  <thead>
                    <tr>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Name</th>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Key Hash</th>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Created</th>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {loading ? (
                      <tr>
                        <td colSpan={4} className="px-4 py-4 text-center text-gray-500 italic">Loading API keys...</td>
                      </tr>
                    ) : apiKeys.length === 0 ? (
                      <tr>
                        <td colSpan={4} className="px-4 py-4 text-center text-gray-500 italic">No API keys found</td>
                      </tr>
                    ) : (
                      apiKeys.map((key) => (
                        <tr key={key.id} className="hover:bg-gray-50">
                          <td className="px-4 py-4 border-b border-gray-200">{key.name}</td>
                          <td className="px-4 py-4 border-b border-gray-200 font-mono text-sm">{key.key_hash.substring(0, 16)}...</td>
                          <td className="px-4 py-4 border-b border-gray-200">{formatDate(key.created_at)}</td>
                          <td className="px-4 py-4 border-b border-gray-200">
                            <button 
                              onClick={() => deleteApiKey(key.id)}
                              className="text-red-600 hover:text-red-800 font-medium"
                            >
                              Delete
                            </button>
                          </td>
                        </tr>
                      ))
                    )}
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {/* Templates Tab */}
          {activeTab === 'templates' && (
            <div>
              <div className="flex justify-between items-center mb-6">
                <h3 className="text-lg font-semibold text-gray-800">Template Management</h3>
                <button className="inline-flex items-center justify-center gap-2 px-6 py-3 bg-blue-600 text-white rounded-md font-medium hover:bg-blue-700 transition-all duration-200">
                  Create Template
                </button>
              </div>

              <div className="bg-white rounded-md shadow-sm overflow-hidden">
                <table className="w-full border-collapse">
                  <thead>
                    <tr>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Name</th>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Type</th>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Default</th>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Created</th>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {loading ? (
                      <tr>
                        <td colSpan={5} className="px-4 py-4 text-center text-gray-500 italic">Loading templates...</td>
                      </tr>
                    ) : templates.length === 0 ? (
                      <tr>
                        <td colSpan={5} className="px-4 py-4 text-center text-gray-500 italic">No templates found</td>
                      </tr>
                    ) : (
                      templates.map((template) => (
                        <tr key={template.id} className="hover:bg-gray-50">
                          <td className="px-4 py-4 border-b border-gray-200">{template.name}</td>
                          <td className="px-4 py-4 border-b border-gray-200">{template.type}</td>
                          <td className="px-4 py-4 border-b border-gray-200">
                            {template.is_default ? (
                              <span className="px-2 py-1 bg-blue-100 text-blue-800 rounded-full text-xs">Default</span>
                            ) : (
                              <span className="px-2 py-1 bg-gray-100 text-gray-600 rounded-full text-xs">Custom</span>
                            )}
                          </td>
                          <td className="px-4 py-4 border-b border-gray-200">{formatDate(template.created_at)}</td>
                          <td className="px-4 py-4 border-b border-gray-200">
                            <button 
                              onClick={() => deleteTemplate(template.id)}
                              className="text-red-600 hover:text-red-800 font-medium mr-4"
                            >
                              Delete
                            </button>
                            <button className="text-blue-600 hover:text-blue-800 font-medium">
                              Edit
                            </button>
                          </td>
                        </tr>
                      ))
                    )}
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {/* System Tab */}
          {activeTab === 'system' && (
            <div>
              <div className="flex justify-between items-center mb-6">
                <h3 className="text-lg font-semibold text-gray-800">System Information</h3>
              </div>

              <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                <div className="bg-white p-6 rounded-lg shadow-sm">
                  <h4 className="text-lg font-semibold mb-4">Server Status</h4>
                  <div className="space-y-2">
                    <div className="flex justify-between">
                      <span>Status:</span>
                      <span className="text-green-600 font-medium">🟢 Online</span>
                    </div>
                    <div className="flex justify-between">
                      <span>Version:</span>
                      <span>Glimpser v0.1.0</span>
                    </div>
                    <div className="flex justify-between">
                      <span>Uptime:</span>
                      <span>Running</span>
                    </div>
                  </div>
                </div>

                <div className="bg-white p-6 rounded-lg shadow-sm">
                  <h4 className="text-lg font-semibold mb-4">Database</h4>
                  <div className="space-y-2">
                    <div className="flex justify-between">
                      <span>Type:</span>
                      <span>SQLite</span>
                    </div>
                    <div className="flex justify-between">
                      <span>Users:</span>
                      <span>{users.length}</span>
                    </div>
                    <div className="flex justify-between">
                      <span>Templates:</span>
                      <span>{templates.length}</span>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          )}
        </main>
      </div>
    </ProtectedRoute>
  );
}