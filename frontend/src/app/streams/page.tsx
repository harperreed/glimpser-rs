//! ABOUTME: Stream dashboard page with grid display and real-time updates
//! ABOUTME: Replaces static/js/streams.js with React components

'use client';

import { useState, useEffect, useCallback, useRef } from 'react';
import { ProtectedRoute } from '@/components/ProtectedRoute';
import { apiClient } from '@/lib/api';
import { useAuth } from '@/contexts/auth';
import { useRouter } from 'next/navigation';

interface Stream {
  id: string;
  template_id?: string;
  name: string;
  status: 'active' | 'inactive';
  last_frame_at?: string;
}

interface Template {
  id: string;
  name: string;
  description?: string;
  config: any;
  is_default: boolean;
  created_by?: string;
  created_at: string;
  updated_at: string;
}

type FilterType = '' | 'active' | 'inactive';
type ViewMode = 'streams' | 'templates';

export default function StreamsPage() {
  const [streams, setStreams] = useState<Stream[]>([]);
  const [filteredStreams, setFilteredStreams] = useState<Stream[]>([]);
  const [templates, setTemplates] = useState<Template[]>([]);
  const [filter, setFilter] = useState<FilterType>('');
  const [viewMode, setViewMode] = useState<ViewMode>('streams');
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedStream, setSelectedStream] = useState<Stream | null>(null);
  const [selectedTemplate, setSelectedTemplate] = useState<Template | null>(null);
  const [showTemplateModal, setShowTemplateModal] = useState(false);
  const [templateModalMode, setTemplateModalMode] = useState<'create' | 'edit' | 'view'>('create');

  const { user, logout } = useAuth();
  const router = useRouter();

  const loadStreams = useCallback(async () => {
    try {
      setError(null);
      const data = await apiClient.streams();
      const streamArray = Array.isArray(data) ? data : [];
      setStreams(streamArray);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load streams');
      setStreams([]);
    } finally {
      setIsLoading(false);
    }
  }, []);

  const loadTemplates = useCallback(async () => {
    try {
      setError(null);
      const data = await apiClient.getTemplates();
      const templateArray = Array.isArray(data?.templates) ? data.templates : [];
      setTemplates(templateArray);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load templates');
      setTemplates([]);
    } finally {
      setIsLoading(false);
    }
  }, []);

  const handleStartStream = async (streamId: string) => {
    try {
      await apiClient.startStream(streamId);
      loadStreams(); // Refresh streams
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to start stream');
    }
  };

  const handleStopStream = async (streamId: string) => {
    try {
      await apiClient.stopStream(streamId);
      loadStreams(); // Refresh streams
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to stop stream');
    }
  };

  const handleDeleteTemplate = async (templateId: string) => {
    if (!confirm('Are you sure you want to delete this template?')) return;
    try {
      await apiClient.deleteTemplate(templateId);
      loadTemplates(); // Refresh templates
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete template');
    }
  };

  const openTemplateModal = (mode: 'create' | 'edit' | 'view', template?: Template) => {
    setTemplateModalMode(mode);
    setSelectedTemplate(template || null);
    setShowTemplateModal(true);
  };

  // Add ref to track if component is mounted
  const isMountedRef = useRef(true);
  useEffect(() => {
    return () => {
      isMountedRef.current = false;
    };
  }, []);

  // Filter streams when filter or streams change
  useEffect(() => {
    if (filter === '') {
      setFilteredStreams(streams);
    } else {
      setFilteredStreams(streams.filter(stream => stream.status === filter));
    }
  }, [streams, filter]);

  // Load data based on view mode
  useEffect(() => {
    if (viewMode === 'streams') {
      loadStreams();
    } else {
      loadTemplates();
    }
  }, [viewMode, loadStreams, loadTemplates]);

  // Auto-refresh every 10 seconds
  useEffect(() => {
    const interval = setInterval(loadStreams, 10000);
    return () => clearInterval(interval);
  }, [loadStreams]);

  // Handle modal keyboard navigation
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && selectedStream) {
        setSelectedStream(null);
      }
    };

    if (selectedStream) {
      document.addEventListener('keydown', handleKeyDown);
      return () => document.removeEventListener('keydown', handleKeyDown);
    }
  }, [selectedStream]);

  const handleLogout = () => {
    logout();
    router.push('/login');
  };

  const formatTimeAgo = (date: Date) => {
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMinutes = Math.floor(diffMs / (1000 * 60));

    if (diffMinutes < 1) return 'Just now';
    if (diffMinutes < 60) return `${diffMinutes}m ago`;

    const diffHours = Math.floor(diffMinutes / 60);
    if (diffHours < 24) return `${diffHours}h ago`;

    const diffDays = Math.floor(diffHours / 24);
    return `${diffDays}d ago`;
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
            <div className="flex items-center gap-2 bg-gray-100 rounded-lg p-1">
              <button
                onClick={() => setViewMode('streams')}
                className={`px-4 py-2 text-sm font-medium rounded-md transition-all duration-200 ${
                  viewMode === 'streams'
                    ? 'bg-white text-blue-600 shadow-sm'
                    : 'text-gray-500 hover:text-blue-600'
                }`}
              >
                Streams
              </button>
              <button
                onClick={() => setViewMode('templates')}
                className={`px-4 py-2 text-sm font-medium rounded-md transition-all duration-200 ${
                  viewMode === 'templates'
                    ? 'bg-white text-blue-600 shadow-sm'
                    : 'text-gray-500 hover:text-blue-600'
                }`}
              >
                Templates
              </button>
            </div>
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
          <div className="flex flex-col md:flex-row justify-between items-start md:items-center gap-4 mb-8">
            <h2 className="text-2xl font-bold text-gray-800">
              {viewMode === 'streams' ? 'Live Streams' : 'Templates'}
            </h2>
            <div className="flex flex-col sm:flex-row items-stretch sm:items-center gap-4">
              <button
                onClick={viewMode === 'streams' ? loadStreams : loadTemplates}
                disabled={isLoading}
                className="px-6 py-3 bg-gray-500 text-white rounded-md font-medium hover:bg-gray-600 transition-all duration-200 disabled:opacity-50"
              >
                {isLoading ? 'Refreshing...' : 'Refresh'}
              </button>
              {viewMode === 'templates' && (
                <button
                  onClick={() => openTemplateModal('create')}
                  className="px-6 py-3 bg-blue-600 text-white rounded-md font-medium hover:bg-blue-700 transition-all duration-200"
                >
                  Create Template
                </button>
              )}
              {viewMode === 'streams' && (
                <select
                  value={filter}
                  onChange={(e) => setFilter(e.target.value as FilterType)}
                  className="px-3 py-3 border border-gray-300 rounded-md text-base transition-colors duration-200 focus:outline-none focus:border-blue-600 focus:ring-4 focus:ring-blue-100"
                >
                  <option value="">All Streams</option>
                  <option value="active">Active Only</option>
                  <option value="inactive">Inactive Only</option>
                </select>
              )}
            </div>
          </div>

          {/* Error State */}
          {error && (
            <div className="bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-6">
              Failed to load {viewMode}: {error}
            </div>
          )}

          {/* Loading State */}
          {isLoading && ((viewMode === 'streams' && streams.length === 0) || (viewMode === 'templates' && templates.length === 0)) && (
            <div className="flex flex-col items-center justify-center min-h-48 bg-white rounded-md shadow-sm text-gray-500">
              <div className="w-8 h-8 border-2 border-transparent border-t-current rounded-full animate-spin mb-4"></div>
              <p>Loading {viewMode}...</p>
            </div>
          )}

          {/* Empty State */}
          {!isLoading && !error && (
            (viewMode === 'streams' && filteredStreams.length === 0) ? (
              <div className="flex flex-col items-center justify-center min-h-48 bg-white rounded-md shadow-sm text-gray-500">
                <p className="text-lg mb-2">📹 No streams found</p>
                <p className="text-sm">Try adjusting your filter or check back later.</p>
              </div>
            ) : (viewMode === 'templates' && templates.length === 0) ? (
              <div className="flex flex-col items-center justify-center min-h-48 bg-white rounded-md shadow-sm text-gray-500">
                <p className="text-lg mb-2">📝 No templates found</p>
                <p className="text-sm">Create your first surveillance template to get started.</p>
                <button
                  onClick={() => openTemplateModal('create')}
                  className="mt-4 px-6 py-3 bg-blue-600 text-white rounded-md font-medium hover:bg-blue-700 transition-all duration-200"
                >
                  Create Template
                </button>
              </div>
            ) : null
          )}

          {/* Streams Grid */}
          {filteredStreams.length > 0 && (
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
              {filteredStreams.map((stream) => (
                <div
                  key={stream.id}
                  className="bg-white rounded-lg shadow-sm border hover:shadow-md transition-shadow duration-200 overflow-hidden"
                >
                  {/* Stream Preview */}
                  <div
                    className="aspect-video bg-gray-100 flex items-center justify-center cursor-pointer hover:bg-gray-200 transition-colors relative group"
                    onClick={() => router.push(`/streams/${stream.id}`)}
                  >
                    {/* View Details Button */}
                    <div className="absolute top-2 right-2 opacity-0 group-hover:opacity-100 transition-opacity">
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          setSelectedStream(stream);
                        }}
                        className="bg-black bg-opacity-50 text-white p-1 rounded text-xs hover:bg-opacity-75"
                        title="View details"
                      >
                        ℹ️
                      </button>
                    </div>
                    {stream.status === 'active' ? (
                      <img
                        src={`/api/stream/${stream.template_id || stream.id}/thumbnail`}
                        alt={stream.name}
                        className="w-full h-full object-cover"
                        onError={(e) => {
                          const img = e.target as HTMLImageElement;
                          img.src = 'data:image/svg+xml;base64,PHN2ZyB3aWR0aD0iMjQiIGhlaWdodD0iMjQiIHZpZXdCb3g9IjAgMCAyNCAyNCIgZmlsbD0ibm9uZSIgeG1sbnM9Imh0dHA6Ly93d3cudzMub3JnLzIwMDAvc3ZnIj4KPHJlY3Qgd2lkdGg9IjI0IiBoZWlnaHQ9IjI0IiBmaWxsPSIjRjNGNEY2Ii8+CjxwYXRoIGQ9Ik0xMiA5VjEzIiBzdHJva2U9IiM2QjcyODAiIHN0cm9rZS13aWR0aD0iMiIgc3Ryb2tlLWxpbmVjYXA9InJvdW5kIi8+CjxwYXRoIGQ9Ik0xMiAxN0gxMi4wMSIgc3Ryb2tlPSIjNkI3MjgwIiBzdHJva2Utd2lkdGg9IjIiIHN0cm9rZS1saW5lY2FwPSJyb3VuZCIvPgo8L3N2Zz4K';
                          img.alt = 'Failed to load thumbnail';
                        }}
                      />
                    ) : (
                      <span className="text-2xl">📹 Offline</span>
                    )}
                  </div>

                  {/* Stream Info */}
                  <div className="p-4">
                    <div className="flex justify-between items-start mb-2">
                      <h3 className="font-semibold text-gray-800 truncate">{stream.name}</h3>
                      <span className={`px-2 py-1 text-xs rounded-full ${
                        stream.status === 'active'
                          ? 'bg-green-100 text-green-800'
                          : 'bg-gray-100 text-gray-600'
                      }`}>
                        {stream.status === 'active' ? 'Online' : 'Offline'}
                      </span>
                    </div>
                    <div className="flex justify-between items-center">
                      <p className="text-sm text-gray-500">
                        Last seen: {
                          stream.last_frame_at
                            ? formatTimeAgo(new Date(stream.last_frame_at))
                            : 'Never'
                        }
                      </p>
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          router.push(`/streams/${stream.id}`);
                        }}
                        className="text-blue-600 hover:text-blue-800 text-sm font-medium"
                      >
                        View →
                      </button>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}

          {/* Templates Grid */}
          {viewMode === 'templates' && templates.length > 0 && (
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
              {templates.map((template) => (
                <div
                  key={template.id}
                  className="bg-white rounded-lg shadow-sm border hover:shadow-md transition-shadow duration-200 overflow-hidden"
                >
                  <div className="p-6">
                    <div className="flex items-start justify-between mb-3">
                      <h3 className="font-semibold text-lg text-gray-800 truncate pr-2">
                        {template.name}
                      </h3>
                      <div className="flex items-center gap-2 flex-shrink-0">
                        {template.is_default && (
                          <span className="px-2 py-1 bg-blue-100 text-blue-800 text-xs font-medium rounded-full">
                            Default
                          </span>
                        )}
                        <div className="flex items-center gap-1">
                          <button
                            onClick={() => openTemplateModal('view', template)}
                            className="p-1 text-gray-400 hover:text-blue-600 transition-colors"
                            title="View template"
                          >
                            <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M2.458 12C3.732 7.943 7.523 5 12 5c4.478 0 8.268 2.943 9.542 7-1.274 4.057-5.064 7-9.542 7-4.477 0-8.268-2.943-9.542-7z" />
                            </svg>
                          </button>
                          <button
                            onClick={() => openTemplateModal('edit', template)}
                            className="p-1 text-gray-400 hover:text-amber-600 transition-colors"
                            title="Edit template"
                          >
                            <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
                            </svg>
                          </button>
                          <button
                            onClick={() => handleDeleteTemplate(template.id)}
                            className="p-1 text-gray-400 hover:text-red-600 transition-colors"
                            title="Delete template"
                          >
                            <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                            </svg>
                          </button>
                        </div>
                      </div>
                    </div>

                    {template.description && (
                      <p className="text-sm text-gray-600 mb-4 line-clamp-2">
                        {template.description}
                      </p>
                    )}

                    <div className="flex items-center justify-between text-sm text-gray-500 mb-4">
                      <span>
                        Created: {new Date(template.created_at).toLocaleDateString()}
                      </span>
                      <span>
                        Updated: {new Date(template.updated_at).toLocaleDateString()}
                      </span>
                    </div>

                    <div className="flex gap-2">
                      <button
                        onClick={async () => {
                          // Start a stream from this template - need to implement backend endpoint
                          try {
                            const response = await apiClient.post(`/templates/${template.id}/start`);
                            loadStreams(); // Refresh to show new stream
                            setViewMode('streams'); // Switch to streams view
                          } catch (err) {
                            setError(err instanceof Error ? err.message : 'Failed to start template');
                          }
                        }}
                        className="flex-1 px-4 py-2 bg-green-600 text-white rounded-md hover:bg-green-700 transition-all duration-200 text-sm font-medium"
                      >
                        Start Stream
                      </button>
                      <button
                        onClick={() => openTemplateModal('view', template)}
                        className="px-4 py-2 border border-gray-300 text-gray-700 rounded-md hover:bg-gray-50 transition-all duration-200 text-sm font-medium"
                      >
                        View Details
                      </button>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}

          {/* Stream Details Modal */}
          {selectedStream && (
            <div
              className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center p-4 z-50"
              onClick={() => setSelectedStream(null)}
            >
              <div
                className="bg-white rounded-lg shadow-xl max-w-2xl w-full max-h-[90vh] overflow-auto"
                onClick={(e) => e.stopPropagation()}
              >
                <div className="p-6">
                  <div className="flex justify-between items-center mb-4">
                    <h3 className="text-lg font-semibold">{selectedStream.name}</h3>
                    <button
                      onClick={() => setSelectedStream(null)}
                      className="text-gray-400 hover:text-gray-600"
                    >
                      ✕
                    </button>
                  </div>

                  <div className="aspect-video bg-gray-100 rounded-lg mb-4 flex items-center justify-center">
                    {selectedStream.status === 'active' ? (
                      <img
                        src={`/api/stream/${selectedStream.template_id || selectedStream.id}/mjpeg`}
                        alt={`${selectedStream.name} live stream`}
                        className="w-full h-full object-cover rounded-lg"
                      />
                    ) : (
                      <span className="text-4xl">📹 Stream Offline</span>
                    )}
                  </div>

                  <div className="space-y-2 mb-6">
                    <p><strong>Status:</strong> <span className={selectedStream.status === 'active' ? 'text-green-600' : 'text-gray-500'}>{selectedStream.status}</span></p>
                    <p><strong>Stream ID:</strong> {selectedStream.id}</p>
                    <p><strong>Last Frame:</strong> {selectedStream.last_frame_at ? formatTimeAgo(new Date(selectedStream.last_frame_at)) : 'Never'}</p>
                  </div>

                  {/* Action Buttons */}
                  <div className="flex justify-end space-x-3">
                    <button
                      onClick={() => setSelectedStream(null)}
                      className="px-4 py-2 border border-gray-300 text-gray-700 rounded-md hover:bg-gray-50"
                    >
                      Close
                    </button>
                    <button
                      onClick={() => {
                        setSelectedStream(null);
                        router.push(`/streams/${selectedStream.id}`);
                      }}
                      className="px-4 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700 flex items-center space-x-2"
                    >
                      <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
                      </svg>
                      <span>View Full Stream</span>
                    </button>
                  </div>
                </div>
              </div>
            </div>
          )}

          {/* Template Modal */}
          {showTemplateModal && (
            <TemplateModal
              mode={templateModalMode}
              template={selectedTemplate}
              onClose={() => {
                setShowTemplateModal(false);
                setSelectedTemplate(null);
              }}
              onSave={async (templateData) => {
                try {
                  if (templateModalMode === 'create') {
                    await apiClient.createTemplate(templateData);
                  } else if (templateModalMode === 'edit' && selectedTemplate) {
                    await apiClient.updateTemplate(selectedTemplate.id, templateData);
                  }
                  loadTemplates(); // Refresh templates
                  setShowTemplateModal(false);
                  setSelectedTemplate(null);
                } catch (err) {
                  setError(err instanceof Error ? err.message : 'Failed to save template');
                }
              }}
            />
          )}
        </main>
      </div>
    </ProtectedRoute>
  );
}

// Template Modal Component
interface TemplateModalProps {
  mode: 'create' | 'edit' | 'view';
  template: Template | null;
  onClose: () => void;
  onSave: (templateData: { name: string; description?: string; config: any; is_default?: boolean }) => void;
}

function TemplateModal({ mode, template, onClose, onSave }: TemplateModalProps) {
  const [formData, setFormData] = useState({
    name: template?.name || '',
    description: template?.description || '',
    config: template?.config || {
      source: {
        type: 'website', // website, rtsp, file
        url: '',
        interval: 60, // seconds
      },
      capture: {
        resolution: '1920x1080',
        format: 'jpg',
      },
      detection: {
        enabled: false,
        threshold: 0.7,
      }
    },
    is_default: template?.is_default || false,
  });

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (mode !== 'view') {
      onSave(formData);
    }
  };

  const isReadOnly = mode === 'view';

  return (
    <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center p-4 z-50">
      <div className="bg-white rounded-lg shadow-xl max-w-2xl w-full max-h-[90vh] overflow-auto">
        <form onSubmit={handleSubmit}>
          <div className="p-6 border-b">
            <div className="flex justify-between items-center">
              <h3 className="text-lg font-semibold">
                {mode === 'create' ? 'Create Template' : mode === 'edit' ? 'Edit Template' : 'View Template'}
              </h3>
              <button
                type="button"
                onClick={onClose}
                className="text-gray-400 hover:text-gray-600"
              >
                ✕
              </button>
            </div>
          </div>

          <div className="p-6 space-y-4">
            {/* Basic Info */}
            <div className="grid grid-cols-1 gap-4">
              <div>
                <label className="block text-sm font-medium text-gray-700 mb-2">
                  Template Name *
                </label>
                <input
                  type="text"
                  value={formData.name}
                  onChange={(e) => setFormData({ ...formData, name: e.target.value })}
                  disabled={isReadOnly}
                  required
                  className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:border-blue-600 focus:ring-1 focus:ring-blue-600 disabled:bg-gray-100"
                  placeholder="Enter template name"
                />
              </div>

              <div>
                <label className="block text-sm font-medium text-gray-700 mb-2">
                  Description
                </label>
                <textarea
                  value={formData.description}
                  onChange={(e) => setFormData({ ...formData, description: e.target.value })}
                  disabled={isReadOnly}
                  rows={3}
                  className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:border-blue-600 focus:ring-1 focus:ring-blue-600 disabled:bg-gray-100"
                  placeholder="Enter template description"
                />
              </div>
            </div>

            {/* Source Configuration */}
            <div className="border-t pt-4">
              <h4 className="font-medium text-gray-800 mb-3">Source Configuration</h4>

              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Source Type *
                  </label>
                  <select
                    value={formData.config.source?.type || 'website'}
                    onChange={(e) => setFormData({
                      ...formData,
                      config: {
                        ...formData.config,
                        source: { ...formData.config.source, type: e.target.value }
                      }
                    })}
                    disabled={isReadOnly}
                    className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:border-blue-600 focus:ring-1 focus:ring-blue-600 disabled:bg-gray-100"
                  >
                    <option value="website">Website</option>
                    <option value="rtsp">RTSP Stream</option>
                    <option value="file">File/Video</option>
                  </select>
                </div>

                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    {formData.config.source?.type === 'website' ? 'Website URL *' :
                     formData.config.source?.type === 'rtsp' ? 'RTSP URL *' : 'File Path *'}
                  </label>
                  <input
                    type="text"
                    value={formData.config.source?.url || ''}
                    onChange={(e) => setFormData({
                      ...formData,
                      config: {
                        ...formData.config,
                        source: { ...formData.config.source, url: e.target.value }
                      }
                    })}
                    disabled={isReadOnly}
                    required
                    className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:border-blue-600 focus:ring-1 focus:ring-blue-600 disabled:bg-gray-100"
                    placeholder={formData.config.source?.type === 'website' ? 'https://example.com' :
                               formData.config.source?.type === 'rtsp' ? 'rtsp://camera-ip/stream' : '/path/to/video.mp4'}
                  />
                </div>

                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Capture Interval (seconds)
                  </label>
                  <input
                    type="number"
                    min="1"
                    max="3600"
                    value={formData.config.source?.interval || 60}
                    onChange={(e) => setFormData({
                      ...formData,
                      config: {
                        ...formData.config,
                        source: { ...formData.config.source, interval: parseInt(e.target.value) }
                      }
                    })}
                    disabled={isReadOnly}
                    className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:border-blue-600 focus:ring-1 focus:ring-blue-600 disabled:bg-gray-100"
                  />
                </div>

                <div>
                  <label className="block text-sm font-medium text-gray-700 mb-2">
                    Resolution
                  </label>
                  <select
                    value={formData.config.capture?.resolution || '1920x1080'}
                    onChange={(e) => setFormData({
                      ...formData,
                      config: {
                        ...formData.config,
                        capture: { ...formData.config.capture, resolution: e.target.value }
                      }
                    })}
                    disabled={isReadOnly}
                    className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:border-blue-600 focus:ring-1 focus:ring-blue-600 disabled:bg-gray-100"
                  >
                    <option value="1920x1080">1920x1080 (Full HD)</option>
                    <option value="1280x720">1280x720 (HD)</option>
                    <option value="800x600">800x600 (SVGA)</option>
                    <option value="640x480">640x480 (VGA)</option>
                  </select>
                </div>
              </div>
            </div>

            {/* Default Template Toggle */}
            <div className="border-t pt-4">
              <label className="flex items-center space-x-3">
                <input
                  type="checkbox"
                  checked={formData.is_default}
                  onChange={(e) => setFormData({ ...formData, is_default: e.target.checked })}
                  disabled={isReadOnly}
                  className="w-4 h-4 text-blue-600 border-gray-300 rounded focus:ring-blue-500 disabled:opacity-50"
                />
                <span className="text-sm font-medium text-gray-700">
                  Set as default template
                </span>
              </label>
            </div>
          </div>

          {/* Modal Footer */}
          <div className="px-6 py-4 border-t bg-gray-50 flex justify-end gap-3">
            <button
              type="button"
              onClick={onClose}
              className="px-4 py-2 border border-gray-300 text-gray-700 rounded-md hover:bg-gray-50 transition-all duration-200"
            >
              {mode === 'view' ? 'Close' : 'Cancel'}
            </button>
            {mode !== 'view' && (
              <button
                type="submit"
                className="px-6 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700 transition-all duration-200"
              >
                {mode === 'create' ? 'Create Template' : 'Save Changes'}
              </button>
            )}
          </div>
        </form>
      </div>
    </div>
  );
}
