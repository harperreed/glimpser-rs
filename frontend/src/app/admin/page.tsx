//! ABOUTME: Admin panel for managing users, API keys, streams, and system
//! ABOUTME: Replaces static/js/admin.js with React components

'use client';

import React, { useState, useEffect, useCallback } from 'react';
import { ProtectedRoute } from '@/components/ProtectedRoute';
import { Navigation } from '@/components/Navigation';
import { RouteErrorBoundary } from '@/components/RouteErrorBoundary';
import { DataErrorBoundary } from '@/components/DataErrorBoundary';
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

interface StreamConfig {
  kind: string;
  source_url?: string;
  file_path?: string;
  url?: string;
  width?: number;
  height?: number;
  [key: string]: unknown;
}

interface Stream {
  id: string;
  user_id: string;
  name: string;
  description?: string;
  type?: string; // Backend returns 'type' not 'config'
  config?: StreamConfig; // Only present when fetching individual stream
  is_default: boolean;
  status?: 'active' | 'inactive';
  execution_status?: string;
  execution_error?: string;
  last_execution?: string;
  created_at: string;
  updated_at: string;
}

type ActiveTab = 'users' | 'api-keys' | 'streams' | 'system';

export default function AdminPage() {
  const [activeTab, setActiveTab] = useState<ActiveTab>('users');
  const [users, setUsers] = useState<User[]>([]);
  const [apiKeys, setApiKeys] = useState<ApiKey[]>([]);
  const [streams, setStreams] = useState<Stream[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showCreateStreamModal, setShowCreateStreamModal] = useState(false);
  const [showEditStreamModal, setShowEditStreamModal] = useState(false);
  const [editingStream, setEditingStream] = useState<Stream | null>(null);
  const [showImportModal, setShowImportModal] = useState(false);
  const [importData, setImportData] = useState('');
  const [importMode, setImportMode] = useState<'skip' | 'overwrite' | 'create_new'>('skip');

  const { user } = useAuth();
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

  const loadStreams = useCallback(async () => {
    try {
      setError(null);
      const data = await apiClient.get('/settings/streams');
      setStreams(Array.isArray(data) ? data : []);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load streams');
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
        case 'streams':
          await loadStreams();
          break;
        case 'system':
          // System tab doesn't need API calls
          break;
      }
    } finally {
      setLoading(false);
    }
  }, [loadUsers, loadApiKeys, loadStreams]);

  useEffect(() => {
    loadTabData(activeTab);
  }, [activeTab, loadTabData]);

  const handleStartStream = async (streamId: string) => {
    try {
      setLoading(true);
      await apiClient.post(`/stream/${streamId}/start`);
      await loadStreams();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to start stream');
    } finally {
      setLoading(false);
    }
  };

  const handleStopStream = async (streamId: string) => {
    try {
      setLoading(true);
      await apiClient.post(`/stream/${streamId}/stop`);
      await loadStreams();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to stop stream');
    } finally {
      setLoading(false);
    }
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

  const deleteStream = async (streamId: string) => {
    if (!confirm('Are you sure you want to delete this stream?')) return;

    try {
      await apiClient.delete(`/settings/streams/${streamId}`);
      await loadStreams();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete stream');
    }
  };

  const exportStreams = async () => {
    try {
      const data = await apiClient.exportStreams();

      // Create download link
      const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `streams_export_${new Date().toISOString().split('T')[0]}.json`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to export streams');
    }
  };

  const importStreams = async () => {
    try {
      const jsonData = JSON.parse(importData);

      if (!jsonData.streams || !Array.isArray(jsonData.streams)) {
        throw new Error('Invalid export format - missing streams array');
      }

      const result = await apiClient.importStreams(jsonData.streams, importMode) as {
        imported?: number;
        skipped?: number;
        errors?: unknown[];
      };

      setShowImportModal(false);
      setImportData('');
      await loadStreams();

      alert(`Import complete: ${result.imported || 0} imported, ${result.skipped || 0} skipped, ${result.errors?.length || 0} errors`);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to import streams');
    }
  };

  const handleFileUpload = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (file) {
      const reader = new FileReader();
      reader.onload = (e) => {
        setImportData(e.target?.result as string);
      };
      reader.readAsText(file);
    }
  };

  const createStream = async (streamData: {
    name: string;
    description?: string;
    config: StreamConfig;
    is_default: boolean;
  }) => {
    try {
      await apiClient.post('/settings/streams', streamData);
      await loadStreams();
      setShowCreateStreamModal(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to create stream');
    }
  };

  const updateStream = async (streamId: string, streamData: {
    name?: string;
    description?: string;
    config?: StreamConfig;
    is_default?: boolean;
  }) => {
    try {
      await apiClient.put(`/settings/streams/${streamId}`, streamData);
      await loadStreams();
      setShowEditStreamModal(false);
      setEditingStream(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to update stream');
    }
  };

  const handleEditStream = async (stream: Stream) => {
    try {
      // Fetch full stream details including config
      const fullStream = await apiClient.get<Stream>(`/settings/streams/${stream.id}`);
      // Parse the config if it's a string
      if (typeof fullStream.config === 'string') {
        fullStream.config = JSON.parse(fullStream.config);
      }
      setEditingStream(fullStream);
      setShowEditStreamModal(true);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load stream details');
    }
  };

  return (
    <ProtectedRoute>
      <RouteErrorBoundary routeName="Admin Panel">
        <div className="min-h-screen bg-slate-50">
        {/* Header */}
        <Navigation />

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
              { key: 'streams', label: 'Streams' },
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

          {/* Streams Tab */}
          {activeTab === 'streams' && (
            <div>
              <div className="flex justify-between items-center mb-6">
                <h3 className="text-lg font-semibold text-gray-800">Stream Management</h3>
                <div className="flex gap-2">
                  <button
                    onClick={exportStreams}
                    className="inline-flex items-center justify-center gap-2 px-6 py-3 bg-green-600 text-white rounded-md font-medium hover:bg-green-700 transition-all duration-200">
                    <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 10v6m0 0l-3-3m3 3l3-3m2 8H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
                    </svg>
                    Export
                  </button>
                  <button
                    onClick={() => setShowImportModal(true)}
                    className="inline-flex items-center justify-center gap-2 px-6 py-3 bg-purple-600 text-white rounded-md font-medium hover:bg-purple-700 transition-all duration-200">
                    <svg className="w-5 h-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M7 16a4 4 0 01-.88-7.903A5 5 0 1115.9 6L16 6a5 5 0 011 9.9M15 13l-3-3m0 0l-3 3m3-3v12" />
                    </svg>
                    Import
                  </button>
                  <button
                    onClick={() => setShowCreateStreamModal(true)}
                    className="inline-flex items-center justify-center gap-2 px-6 py-3 bg-blue-600 text-white rounded-md font-medium hover:bg-blue-700 transition-all duration-200">
                    Create Stream
                  </button>
                </div>
              </div>

              <div className="bg-white rounded-md shadow-sm overflow-hidden">
                <table className="w-full border-collapse">
                  <thead>
                    <tr>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Name</th>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Type</th>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Status</th>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Default</th>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Created</th>
                      <th className="px-4 py-4 text-left border-b border-gray-300 bg-slate-50 font-semibold text-gray-800">Actions</th>
                    </tr>
                  </thead>
                  <tbody>
                    {loading ? (
                      <tr>
                        <td colSpan={6} className="px-4 py-4 text-center text-gray-500 italic">Loading streams...</td>
                      </tr>
                    ) : streams.length === 0 ? (
                      <tr>
                        <td colSpan={6} className="px-4 py-4 text-center text-gray-500 italic">No streams found</td>
                      </tr>
                    ) : (
                      streams.map((stream) => (
                        <tr key={stream.id} className="hover:bg-gray-50">
                          <td className="px-4 py-4 border-b border-gray-200">{stream.name}</td>
                          <td className="px-4 py-4 border-b border-gray-200">{stream.type || stream.config?.kind || 'unknown'}</td>
                          <td className="px-4 py-4 border-b border-gray-200">
                            {stream.status === 'active' ? (
                              <span className="px-2 py-1 bg-green-100 text-green-800 rounded-full text-xs">Active</span>
                            ) : (
                              <span className="px-2 py-1 bg-gray-100 text-gray-600 rounded-full text-xs">Inactive</span>
                            )}
                          </td>
                          <td className="px-4 py-4 border-b border-gray-200">
                            {stream.is_default ? (
                              <span className="px-2 py-1 bg-blue-100 text-blue-800 rounded-full text-xs">Default</span>
                            ) : (
                              <span className="px-2 py-1 bg-gray-100 text-gray-600 rounded-full text-xs">Custom</span>
                            )}
                          </td>
                          <td className="px-4 py-4 border-b border-gray-200">{formatDate(stream.created_at)}</td>
                          <td className="px-4 py-4 border-b border-gray-200">
                            <div className="flex gap-2 items-center">
                              <button
                                onClick={(e) => {
                                  e.preventDefault();
                                  e.stopPropagation();
                                  if (stream.status === 'active') {
                                    handleStopStream(stream.id);
                                  } else {
                                    handleStartStream(stream.id);
                                  }
                                }}
                                className={`px-3 py-1 rounded-full text-xs font-medium transition-colors ${
                                  stream.status === 'active'
                                    ? 'bg-red-100 text-red-700 hover:bg-red-200'
                                    : 'bg-green-100 text-green-700 hover:bg-green-200'
                                }`}
                                disabled={loading}
                                type="button">
                                {stream.status === 'active' ? (
                                  <span className="flex items-center gap-1">
                                    <span className="w-2 h-2 bg-red-600 rounded-full animate-pulse"></span>
                                    Stop
                                  </span>
                                ) : (
                                  <span className="flex items-center gap-1">
                                    <span className="w-2 h-2 bg-gray-400 rounded-full"></span>
                                    Start
                                  </span>
                                )}
                              </button>
                              <button
                                onClick={() => handleEditStream(stream)}
                                className="text-blue-600 hover:text-blue-800 font-medium text-sm"
                                disabled={loading}>
                                Edit
                              </button>
                              <button
                                onClick={() => deleteStream(stream.id)}
                                className="text-red-600 hover:text-red-800 font-medium text-sm"
                              >
                                Delete
                              </button>
                            </div>
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
                      <span>Streams:</span>
                      <span>{streams.length}</span>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          )}
        </main>

        {/* Create Stream Modal */}
        {showCreateStreamModal && (
          <StreamFormModal
            isOpen={showCreateStreamModal}
            onClose={() => setShowCreateStreamModal(false)}
            onSubmit={createStream}
            title="Create New Stream"
          />
        )}

        {/* Edit Stream Modal */}
        {showEditStreamModal && editingStream && (
          <StreamFormModal
            isOpen={showEditStreamModal}
            onClose={() => {
              setShowEditStreamModal(false);
              setEditingStream(null);
            }}
            onSubmit={(data) => updateStream(editingStream.id, data)}
            initialData={editingStream}
            title="Edit Stream"
          />
        )}

        {/* Import Streams Modal */}
        {showImportModal && (
          <div className="fixed inset-0 bg-black bg-opacity-50 z-50 flex items-center justify-center">
            <div className="bg-white rounded-lg shadow-xl max-w-2xl w-full mx-4 max-h-[80vh] overflow-y-auto">
              <div className="p-6 border-b border-gray-200">
                <div className="flex justify-between items-center">
                  <h3 className="text-xl font-semibold text-gray-800">Import Streams</h3>
                  <button
                    onClick={() => {
                      setShowImportModal(false);
                      setImportData('');
                    }}
                    className="text-gray-400 hover:text-gray-600">
                    <svg className="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                    </svg>
                  </button>
                </div>
              </div>
              <div className="p-6">
                <div className="mb-4">
                  <label className="block text-sm font-medium text-gray-700 mb-2">Import Mode</label>
                  <select
                    value={importMode}
                    onChange={(e) => setImportMode(e.target.value as 'skip' | 'overwrite' | 'create_new')}
                    className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500">
                    <option value="skip">Skip existing streams (default)</option>
                    <option value="overwrite">Overwrite existing streams</option>
                    <option value="create_new">Create as new streams (append numbers)</option>
                  </select>
                </div>
                <div className="mb-4">
                  <label className="block text-sm font-medium text-gray-700 mb-2">JSON Data</label>
                  <textarea
                    value={importData}
                    onChange={(e) => setImportData(e.target.value)}
                    className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 font-mono text-sm"
                    rows={10}
                    placeholder="Paste your exported JSON data here..."
                  />
                </div>
                <div className="mb-4">
                  <label className="block text-sm font-medium text-gray-700 mb-2">Or upload a file</label>
                  <input
                    type="file"
                    accept=".json"
                    onChange={handleFileUpload}
                    className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                  />
                </div>
                <div className="flex justify-end gap-2">
                  <button
                    onClick={() => {
                      setShowImportModal(false);
                      setImportData('');
                    }}
                    className="px-4 py-2 bg-gray-500 text-white rounded-md hover:bg-gray-600 transition-all duration-200">
                    Cancel
                  </button>
                  <button
                    onClick={importStreams}
                    disabled={!importData}
                    className="px-4 py-2 bg-purple-600 text-white rounded-md hover:bg-purple-700 transition-all duration-200 disabled:opacity-50 disabled:cursor-not-allowed">
                    Import
                  </button>
                </div>
              </div>
            </div>
          </div>
        )}
        </div>
      </RouteErrorBoundary>
    </ProtectedRoute>
  );
}

// Stream Form Modal Component
function StreamFormModal({
  isOpen,
  onClose,
  onSubmit,
  initialData,
  title
}: {
  isOpen: boolean;
  onClose: () => void;
  onSubmit: (data: {
    name: string;
    description?: string;
    config: StreamConfig;
    is_default: boolean;
  }) => void;
  initialData?: Stream;
  title: string;
}) {
  const [configError, setConfigError] = useState<string | null>(null);
  const [showJsonEditor, setShowJsonEditor] = useState(false);

  // Parse initial config if editing
  const parseInitialConfig = () => {
    if (initialData?.config) {
      const cfg = typeof initialData.config === 'string' ? JSON.parse(initialData.config) : initialData.config;
      return cfg;
    }
    return null;
  };

  const initialConfig = parseInitialConfig();

  const [formData, setFormData] = useState({
    name: initialData?.name || '',
    description: initialData?.description || '',
    type: initialData?.type || initialConfig?.kind || 'rtsp',
    is_default: initialData?.is_default || false,
    // Common fields
    snapshot_interval: initialConfig?.snapshot_interval || 5,
    duration: initialConfig?.duration || 300,
    unlimited_duration: initialConfig?.duration === 0 || false,
    // RTSP fields
    rtsp_url: initialConfig?.source_url || '',
    // FFmpeg fields
    ffmpeg_url: initialConfig?.source_url || '',
    // File fields
    file_path: initialConfig?.file_path || '',
    // Website fields
    website_url: initialConfig?.url || '',
    website_selector_type: initialConfig?.selector_type || 'css',
    website_element_selector: initialConfig?.element_selector || '',
    website_headless: initialConfig?.headless ?? true,
    website_stealth: initialConfig?.stealth || false,
    website_auth_username: initialConfig?.basic_auth_username || '',
    website_auth_password: initialConfig?.basic_auth_password || '',
    website_timeout: initialConfig?.timeout || 30,
    // YouTube fields
    youtube_url: initialConfig?.url || '',
    youtube_is_live: initialConfig?.is_live || false,
    youtube_format: initialConfig?.format || 'best',
    // Common dimensions
    width: initialConfig?.width || 1920,
    height: initialConfig?.height || 1080,
    // Raw JSON for advanced mode
    rawConfig: initialData?.config ? JSON.stringify(initialData.config, null, 2) : ''
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();

    // Validate name
    if (!formData.name.trim()) {
      setConfigError('Stream name is required');
      return;
    }

    let config: StreamConfig;

    if (showJsonEditor) {
      // Parse JSON from editor
      try {
        config = JSON.parse(formData.rawConfig);
        if (!config.kind) {
          setConfigError('Configuration must have a "kind" field');
          return;
        }
      } catch (err) {
        setConfigError('Invalid JSON: ' + (err instanceof Error ? err.message : 'Unknown error'));
        return;
      }
    } else {
      // Build config from form fields
      switch (formData.type) {
        case 'rtsp':
          if (!formData.rtsp_url) {
            setConfigError('RTSP URL is required');
            return;
          }
          config = {
            kind: 'rtsp',
            source_url: formData.rtsp_url,
            width: formData.width,
            height: formData.height,
            snapshot_interval: formData.snapshot_interval,
            duration: formData.unlimited_duration ? 0 : formData.duration
          };
          break;

        case 'ffmpeg':
          if (!formData.ffmpeg_url) {
            setConfigError('Stream URL is required');
            return;
          }
          config = {
            kind: 'ffmpeg',
            source_url: formData.ffmpeg_url,
            width: formData.width,
            height: formData.height,
            snapshot_interval: formData.snapshot_interval,
            duration: formData.unlimited_duration ? 0 : formData.duration
          };
          break;

        case 'file':
          if (!formData.file_path) {
            setConfigError('File path is required');
            return;
          }
          config = {
            kind: 'file',
            file_path: formData.file_path,
            snapshot_interval: formData.snapshot_interval,
            duration: formData.unlimited_duration ? 0 : formData.duration
          };
          break;

        case 'website':
          if (!formData.website_url) {
            setConfigError('Website URL is required');
            return;
          }
          config = {
            kind: 'website',
            url: formData.website_url,
            width: formData.width,
            height: formData.height,
            headless: formData.website_headless,
            stealth: formData.website_stealth,
            timeout: formData.website_timeout,
            snapshot_interval: formData.snapshot_interval,
            duration: formData.unlimited_duration ? 0 : formData.duration,
            // Always include selector_type, and include element_selector if provided
            selector_type: formData.website_selector_type || 'css',
            ...(formData.website_element_selector && {
              element_selector: formData.website_element_selector
            }),
            ...(formData.website_auth_username && { basic_auth_username: formData.website_auth_username }),
            ...(formData.website_auth_password && { basic_auth_password: formData.website_auth_password })
          };
          break;

        case 'yt':
          if (!formData.youtube_url) {
            setConfigError('YouTube URL is required');
            return;
          }
          config = {
            kind: 'yt',
            url: formData.youtube_url,
            is_live: formData.youtube_is_live,
            format: formData.youtube_format,
            snapshot_interval: formData.snapshot_interval,
            duration: formData.unlimited_duration ? 0 : formData.duration
          };
          break;

        default:
          setConfigError('Invalid stream type');
          return;
      }
    }

    onSubmit({
      name: formData.name.trim(),
      description: formData.description?.trim() || undefined,
      config,
      is_default: formData.is_default
    });

    onClose();
  };

  const streamTypes = [
    { value: 'rtsp', label: 'RTSP Stream' },
    { value: 'ffmpeg', label: 'FFmpeg Source' },
    { value: 'file', label: 'File Source' },
    { value: 'website', label: 'Website Capture' },
    { value: 'yt', label: 'YouTube Stream' }
  ];

  const handleTypeChange = (type: string) => {
    setFormData({
      ...formData,
      type
    });
    setConfigError(null);
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
      <div className="bg-white rounded-lg p-8 max-w-2xl w-full max-h-[90vh] overflow-y-auto">
        <h2 className="text-2xl font-bold mb-6">{title}</h2>
        <form onSubmit={handleSubmit}>
          <div className="mb-4">
            <label className="block text-sm font-medium text-gray-700 mb-2">
              Stream Name
            </label>
            <input
              type="text"
              value={formData.name}
              onChange={(e) => setFormData({ ...formData, name: e.target.value })}
              className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
              required
            />
          </div>

          <div className="mb-4">
            <label className="block text-sm font-medium text-gray-700 mb-2">
              Description (optional)
            </label>
            <textarea
              value={formData.description}
              onChange={(e) => setFormData({ ...formData, description: e.target.value })}
              className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
              rows={3}
            />
          </div>

          <div className="mb-4">
            <label className="block text-sm font-medium text-gray-700 mb-2">
              Stream Type
            </label>
            <select
              value={formData.type}
              onChange={(e) => handleTypeChange(e.target.value)}
              className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
            >
              {streamTypes.map(type => (
                <option key={type.value} value={type.value}>{type.label}</option>
              ))}
            </select>
          </div>

          {/* Toggle between form and JSON editor */}
          <div className="mb-4">
            <button
              type="button"
              onClick={() => {
                setShowJsonEditor(!showJsonEditor);
                if (!showJsonEditor) {
                  // Convert form to JSON when switching to JSON editor
                  let config: StreamConfig;
                  switch (formData.type) {
                    case 'rtsp':
                      config = {
                        kind: 'rtsp',
                        source_url: formData.rtsp_url || 'rtsp://example.com/stream',
                        width: formData.width,
                        height: formData.height
                      };
                      break;
                    case 'ffmpeg':
                      config = {
                        kind: 'ffmpeg',
                        source_url: formData.ffmpeg_url || 'http://example.com/stream.m3u8',
                        width: formData.width,
                        height: formData.height
                      };
                      break;
                    case 'file':
                      config = {
                        kind: 'file',
                        file_path: formData.file_path || '/path/to/video.mp4'
                      };
                      break;
                    case 'website':
                      config = {
                        kind: 'website',
                        url: formData.website_url || 'https://example.com',
                        width: formData.width,
                        height: formData.height,
                        headless: formData.website_headless,
                        stealth: formData.website_stealth,
                        timeout: formData.website_timeout,
                        selector_type: formData.website_selector_type || 'css',
                        ...(formData.website_element_selector && { element_selector: formData.website_element_selector }),
                        ...(formData.website_auth_username && { basic_auth_username: formData.website_auth_username }),
                        ...(formData.website_auth_password && { basic_auth_password: formData.website_auth_password })
                      };
                      break;
                    case 'yt':
                      config = {
                        kind: 'yt',
                        url: formData.youtube_url || 'https://www.youtube.com/watch?v=VIDEO_ID',
                        is_live: formData.youtube_is_live,
                        format: formData.youtube_format || 'best'
                      };
                      break;
                    default:
                      config = { kind: 'rtsp', source_url: '', width: 1920, height: 1080 };
                  }
                  setFormData({ ...formData, rawConfig: JSON.stringify(config, null, 2) });
                }
              }}
              className="text-sm text-blue-600 hover:text-blue-800 underline"
            >
              {showJsonEditor ? '← Back to Form' : 'Advanced (JSON) →'}
            </button>
          </div>

          {/* Configuration fields based on stream type */}
          {!showJsonEditor ? (
            <div className="space-y-4 mb-6">
              {/* Common fields for all stream types */}
              <div className="grid grid-cols-2 gap-4">
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Snapshot Interval (seconds)
                  </label>
                  <input
                    type="number"
                    value={formData.snapshot_interval}
                    onChange={(e) => setFormData({ ...formData, snapshot_interval: parseInt(e.target.value) || 5 })}
                    min="1"
                    max="3600"
                    className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                  />
                  <p className="mt-1 text-xs text-gray-500">How often to take snapshots</p>
                </div>
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Duration (seconds)
                  </label>
                  <input
                    type="number"
                    value={formData.unlimited_duration ? 0 : formData.duration}
                    onChange={(e) => setFormData({ ...formData, duration: parseInt(e.target.value) || 300 })}
                    min="60"
                    max="86400"
                    disabled={formData.unlimited_duration}
                    className={`w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 ${
                      formData.unlimited_duration ? 'bg-gray-100 text-gray-500' : ''
                    }`}
                  />
                  <label className="flex items-center mt-2">
                    <input
                      type="checkbox"
                      checked={formData.unlimited_duration}
                      onChange={(e) => setFormData({ ...formData, unlimited_duration: e.target.checked })}
                      className="mr-2"
                    />
                    <span className="text-sm text-gray-700">Run indefinitely (no time limit)</span>
                  </label>
                  <p className="mt-1 text-xs text-gray-500">
                    {formData.unlimited_duration ? 'Capture will run until manually stopped' : 'How long to run the capture'}
                  </p>
                </div>
              </div>

              {/* RTSP Configuration */}
              {formData.type === 'rtsp' && (
                <>
                  <div>
                    <label className="block text-sm font-medium text-gray-700 mb-2">
                      RTSP URL *
                    </label>
                    <input
                      type="text"
                      value={formData.rtsp_url}
                      onChange={(e) => setFormData({ ...formData, rtsp_url: e.target.value })}
                      placeholder="rtsp://192.168.1.100:554/stream"
                      className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                    />
                  </div>
                  <div className="grid grid-cols-2 gap-4">
                    <div>
                      <label className="block text-sm font-medium text-gray-700 mb-2">Width</label>
                      <input
                        type="number"
                        value={formData.width}
                        onChange={(e) => setFormData({ ...formData, width: parseInt(e.target.value) || 1920 })}
                        className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                      />
                    </div>
                    <div>
                      <label className="block text-sm font-medium text-gray-700 mb-2">Height</label>
                      <input
                        type="number"
                        value={formData.height}
                        onChange={(e) => setFormData({ ...formData, height: parseInt(e.target.value) || 1080 })}
                        className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                      />
                    </div>
                  </div>
                </>
              )}

              {/* FFmpeg Configuration */}
              {formData.type === 'ffmpeg' && (
                <>
                  <div>
                    <label className="block text-sm font-medium text-gray-700 mb-2">
                      Stream URL *
                    </label>
                    <input
                      type="text"
                      value={formData.ffmpeg_url}
                      onChange={(e) => setFormData({ ...formData, ffmpeg_url: e.target.value })}
                      placeholder="http://example.com/stream.m3u8"
                      className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                    />
                  </div>
                  <div className="grid grid-cols-2 gap-4">
                    <div>
                      <label className="block text-sm font-medium text-gray-700 mb-2">Width</label>
                      <input
                        type="number"
                        value={formData.width}
                        onChange={(e) => setFormData({ ...formData, width: parseInt(e.target.value) || 1920 })}
                        className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                      />
                    </div>
                    <div>
                      <label className="block text-sm font-medium text-gray-700 mb-2">Height</label>
                      <input
                        type="number"
                        value={formData.height}
                        onChange={(e) => setFormData({ ...formData, height: parseInt(e.target.value) || 1080 })}
                        className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                      />
                    </div>
                  </div>
                </>
              )}

              {/* File Configuration */}
              {formData.type === 'file' && (
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    File Path *
                  </label>
                  <input
                    type="text"
                    value={formData.file_path}
                    onChange={(e) => setFormData({ ...formData, file_path: e.target.value })}
                    placeholder="/path/to/video.mp4"
                    className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                  />
                </div>
              )}

              {/* Website Configuration */}
              {formData.type === 'website' && (
                <>
                  <div>
                    <label className="block text-sm font-medium text-gray-700 mb-2">
                      Website URL *
                    </label>
                    <input
                      type="url"
                      value={formData.website_url}
                      onChange={(e) => setFormData({ ...formData, website_url: e.target.value })}
                      placeholder="https://example.com"
                      className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                    />
                  </div>

                  <div>
                    <label className="block text-sm font-medium text-gray-700 mb-2">
                      Element Selector (optional)
                    </label>
                    <div className="grid grid-cols-3 gap-2">
                      <select
                        value={formData.website_selector_type}
                        onChange={(e) => setFormData({ ...formData, website_selector_type: e.target.value })}
                        className="px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                      >
                        <option value="css">CSS</option>
                        <option value="xpath">XPath</option>
                      </select>
                      <input
                        type="text"
                        value={formData.website_element_selector}
                        onChange={(e) => setFormData({ ...formData, website_element_selector: e.target.value })}
                        placeholder={formData.website_selector_type === 'css' ? "#main-content, .video-player" : "//div[@id='content']"}
                        className="col-span-2 px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                      />
                    </div>
                    <p className="mt-1 text-xs text-gray-500">
                      {formData.website_selector_type === 'css'
                        ? "CSS selector to capture specific element (e.g., #content, .main)"
                        : "XPath expression to capture specific element (e.g., //div[@class='main'])"}
                    </p>
                  </div>

                  <div className="grid grid-cols-2 gap-4">
                    <div>
                      <label className="block text-sm font-medium text-gray-700 mb-2">Width</label>
                      <input
                        type="number"
                        value={formData.width}
                        onChange={(e) => setFormData({ ...formData, width: parseInt(e.target.value) || 1920 })}
                        className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                      />
                    </div>
                    <div>
                      <label className="block text-sm font-medium text-gray-700 mb-2">Height</label>
                      <input
                        type="number"
                        value={formData.height}
                        onChange={(e) => setFormData({ ...formData, height: parseInt(e.target.value) || 1080 })}
                        className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                      />
                    </div>
                  </div>

                  {/* Advanced Website Options */}
                  <details className="border border-gray-200 rounded-md p-3">
                    <summary className="cursor-pointer text-sm font-medium text-gray-700">Advanced Options</summary>
                    <div className="mt-3 space-y-3">
                      <div className="grid grid-cols-2 gap-4">
                        <div>
                          <label className="block text-sm font-medium text-gray-700 mb-2">
                            Auth Username
                          </label>
                          <input
                            type="text"
                            value={formData.website_auth_username}
                            onChange={(e) => setFormData({ ...formData, website_auth_username: e.target.value })}
                            placeholder="Optional"
                            className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                          />
                        </div>
                        <div>
                          <label className="block text-sm font-medium text-gray-700 mb-2">
                            Auth Password
                          </label>
                          <input
                            type="password"
                            value={formData.website_auth_password}
                            onChange={(e) => setFormData({ ...formData, website_auth_password: e.target.value })}
                            placeholder="Optional"
                            className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                          />
                        </div>
                      </div>

                      <div>
                        <label className="block text-sm font-medium text-gray-700 mb-2">
                          Timeout (seconds)
                        </label>
                        <input
                          type="number"
                          value={formData.website_timeout}
                          onChange={(e) => setFormData({ ...formData, website_timeout: parseInt(e.target.value) || 30 })}
                          min="1"
                          max="300"
                          className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                        />
                      </div>

                      <div className="space-y-2">
                        <label className="flex items-center">
                          <input
                            type="checkbox"
                            checked={formData.website_headless}
                            onChange={(e) => setFormData({ ...formData, website_headless: e.target.checked })}
                            className="mr-2"
                          />
                          <span className="text-sm font-medium text-gray-700">Run in headless mode</span>
                        </label>

                        <label className="flex items-center">
                          <input
                            type="checkbox"
                            checked={formData.website_stealth}
                            onChange={(e) => setFormData({ ...formData, website_stealth: e.target.checked })}
                            className="mr-2"
                          />
                          <span className="text-sm font-medium text-gray-700">Enable stealth mode (avoid detection)</span>
                        </label>
                      </div>
                    </div>
                  </details>
                </>
              )}

              {/* YouTube Configuration */}
              {formData.type === 'yt' && (
                <>
                  <div>
                    <label className="block text-sm font-medium text-gray-700 mb-2">
                      YouTube URL *
                    </label>
                    <input
                      type="url"
                      value={formData.youtube_url}
                      onChange={(e) => setFormData({ ...formData, youtube_url: e.target.value })}
                      placeholder="https://www.youtube.com/watch?v=VIDEO_ID"
                      className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                    />
                    <p className="mt-1 text-xs text-gray-500">Supports YouTube, Twitch, and other yt-dlp compatible sites</p>
                  </div>

                  <div className="grid grid-cols-2 gap-4">
                    <div>
                      <label className="block text-sm font-medium text-gray-700 mb-2">
                        Quality/Format
                      </label>
                      <select
                        value={formData.youtube_format}
                        onChange={(e) => setFormData({ ...formData, youtube_format: e.target.value })}
                        className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500"
                      >
                        <option value="best">Best Quality</option>
                        <option value="worst">Lowest Quality (faster)</option>
                        <option value="bestvideo">Best Video Only</option>
                        <option value="1080p">1080p</option>
                        <option value="720p">720p</option>
                        <option value="480p">480p</option>
                      </select>
                    </div>

                    <div className="flex items-end">
                      <label className="flex items-center">
                        <input
                          type="checkbox"
                          checked={formData.youtube_is_live}
                          onChange={(e) => setFormData({ ...formData, youtube_is_live: e.target.checked })}
                          className="mr-2"
                        />
                        <span className="text-sm font-medium text-gray-700">Live Stream</span>
                      </label>
                    </div>
                  </div>
                </>
              )}
            </div>
          ) : (
            /* JSON Editor for advanced mode */
            <div className="mb-6">
              <label className="block text-sm font-medium text-gray-700 mb-2">
                Configuration (JSON)
              </label>
              <textarea
                value={formData.rawConfig}
                onChange={(e) => {
                  setFormData({ ...formData, rawConfig: e.target.value });
                  setConfigError(null);
                }}
                className={`w-full px-3 py-2 border rounded-md font-mono text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 ${
                  configError ? 'border-red-500' : 'border-gray-300'
                }`}
                rows={10}
                placeholder={JSON.stringify({ kind: formData.type }, null, 2)}
              />
            </div>
          )}

          <div className="mb-6">
            <label className="flex items-center">
              <input
                type="checkbox"
                checked={formData.is_default}
                onChange={(e) => setFormData({ ...formData, is_default: e.target.checked })}
                className="mr-2"
              />
              <span className="text-sm font-medium text-gray-700">
                Set as default stream
              </span>
            </label>
          </div>

          {configError && (
            <div className="mb-4 p-3 bg-red-50 border border-red-300 rounded-md text-red-700 text-sm">
              {configError}
            </div>
          )}

          <div className="flex justify-end gap-4">
            <button
              type="button"
              onClick={onClose}
              className="px-6 py-2 border border-gray-300 rounded-md hover:bg-gray-50"
            >
              Cancel
            </button>
            <button
              type="submit"
              className="px-6 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700"
            >
              {initialData ? 'Update' : 'Create'} Stream
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
