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


type FilterType = '' | 'active' | 'inactive';

export default function StreamsPage() {
  const [streams, setStreams] = useState<Stream[]>([]);
  const [filteredStreams, setFilteredStreams] = useState<Stream[]>([]);
  const [filter, setFilter] = useState<FilterType>('');
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedStream, setSelectedStream] = useState<Stream | null>(null);

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

  // Load streams on mount
  useEffect(() => {
    loadStreams();
  }, [loadStreams]);

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
              Live Streams
            </h2>
            <div className="flex flex-col sm:flex-row items-stretch sm:items-center gap-4">
              <button
                onClick={loadStreams}
                disabled={isLoading}
                className="px-6 py-3 bg-gray-500 text-white rounded-md font-medium hover:bg-gray-600 transition-all duration-200 disabled:opacity-50"
              >
                {isLoading ? 'Refreshing...' : 'Refresh'}
              </button>
              <select
                value={filter}
                onChange={(e) => setFilter(e.target.value as FilterType)}
                className="px-3 py-3 border border-gray-300 rounded-md text-base transition-colors duration-200 focus:outline-none focus:border-blue-600 focus:ring-4 focus:ring-blue-100"
              >
                <option value="">All Streams</option>
                <option value="active">Active Only</option>
                <option value="inactive">Inactive Only</option>
              </select>
            </div>
          </div>

          {/* Error State */}
          {error && (
            <div className="bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-6">
              Failed to load streams: {error}
            </div>
          )}

          {/* Loading State */}
          {isLoading && streams.length === 0 && (
            <div className="flex flex-col items-center justify-center min-h-48 bg-white rounded-md shadow-sm text-gray-500">
              <div className="w-8 h-8 border-2 border-transparent border-t-current rounded-full animate-spin mb-4"></div>
              <p>Loading streams...</p>
            </div>
          )}

          {/* Empty State */}
          {!isLoading && !error && filteredStreams.length === 0 && (
            <div className="flex flex-col items-center justify-center min-h-48 bg-white rounded-md shadow-sm text-gray-500">
              <p className="text-lg mb-2">📹 No streams found</p>
              <p className="text-sm">Try adjusting your filter or check back later.</p>
            </div>
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

        </main>
      </div>
    </ProtectedRoute>
  );
}
