//! ABOUTME: Stream viewer page for displaying individual MJPEG streams
//! ABOUTME: Provides full-screen viewing experience with stream controls

'use client';

import { useState, useEffect, useRef } from 'react';
import { useRouter, useParams } from 'next/navigation';
import { ProtectedRoute } from '@/components/ProtectedRoute';
import { useAuth } from '@/contexts/auth';
import { apiClient } from '@/lib/api';

interface Stream {
  id: string;
  template_id?: string;
  name: string;
  status: 'active' | 'inactive';
  last_frame_at?: string;
}

export default function StreamViewerPage() {
  const router = useRouter();
  const params = useParams();
  const { user, logout } = useAuth();
  const [stream, setStream] = useState<Stream | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [streamError, setStreamError] = useState(false);
  const videoRef = useRef<HTMLImageElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  const streamId = params.id as string;

  useEffect(() => {
    loadStream();
  }, [streamId]);

  const loadStream = async () => {
    try {
      setError(null);
      setIsLoading(true);
      
      const streams = await apiClient.streams();
      const foundStream = Array.isArray(streams) 
        ? streams.find((s: Stream) => s.id === streamId || s.template_id === streamId)
        : null;

      if (!foundStream) {
        setError('Stream not found');
        return;
      }

      setStream(foundStream);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load stream');
    } finally {
      setIsLoading(false);
    }
  };

  const handleLogout = () => {
    logout();
    router.push('/login');
  };

  const toggleFullscreen = () => {
    if (!isFullscreen && containerRef.current) {
      if (containerRef.current.requestFullscreen) {
        containerRef.current.requestFullscreen();
        setIsFullscreen(true);
      }
    } else if (document.exitFullscreen) {
      document.exitFullscreen();
      setIsFullscreen(false);
    }
  };

  const handleStreamError = () => {
    setStreamError(true);
    setTimeout(() => {
      setStreamError(false);
    }, 5000);
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

  if (isLoading) {
    return (
      <ProtectedRoute>
        <div className="min-h-screen bg-gray-900 flex items-center justify-center">
          <div className="text-center">
            <div className="w-12 h-12 border-4 border-transparent border-t-blue-500 rounded-full animate-spin mx-auto mb-4"></div>
            <p className="text-white">Loading stream...</p>
          </div>
        </div>
      </ProtectedRoute>
    );
  }

  if (error || !stream) {
    return (
      <ProtectedRoute>
        <div className="min-h-screen bg-gray-900 flex items-center justify-center">
          <div className="text-center">
            <div className="text-6xl mb-4">📹</div>
            <h1 className="text-2xl font-bold text-white mb-2">Stream Not Found</h1>
            <p className="text-gray-400 mb-6">{error || 'The requested stream could not be found.'}</p>
            <button
              onClick={() => router.push('/streams')}
              className="bg-blue-600 hover:bg-blue-700 text-white px-6 py-3 rounded-lg font-medium"
            >
              Back to Streams
            </button>
          </div>
        </div>
      </ProtectedRoute>
    );
  }

  return (
    <ProtectedRoute>
      <div ref={containerRef} className={`${isFullscreen ? 'bg-black' : 'min-h-screen bg-gray-900'}`}>
        
        {/* Header - hidden in fullscreen */}
        {!isFullscreen && (
          <header className="bg-gray-800 border-b border-gray-700">
            <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
              <div className="flex justify-between items-center py-4">
                <div className="flex items-center space-x-4">
                  <button
                    onClick={() => router.push('/streams')}
                    className="text-gray-400 hover:text-white"
                  >
                    <svg className="w-6 h-6" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 19l-7-7 7-7" />
                    </svg>
                  </button>
                  <div>
                    <h1 className="text-xl font-bold text-white">{stream.name}</h1>
                    <div className="flex items-center space-x-4 text-sm text-gray-400">
                      <span className={`px-2 py-1 text-xs rounded-full ${
                        stream.status === 'active'
                          ? 'bg-green-900 text-green-300'
                          : 'bg-gray-700 text-gray-300'
                      }`}>
                        {stream.status === 'active' ? 'Live' : 'Offline'}
                      </span>
                      {stream.last_frame_at && (
                        <span>Last frame: {formatTimeAgo(new Date(stream.last_frame_at))}</span>
                      )}
                    </div>
                  </div>
                </div>
                <div className="flex items-center space-x-4">
                  <span className="text-sm text-gray-400">
                    Welcome, {user?.username || user?.email}
                  </span>
                  <button
                    onClick={handleLogout}
                    className="bg-red-600 hover:bg-red-700 text-white px-4 py-2 rounded-md text-sm font-medium"
                  >
                    Logout
                  </button>
                </div>
              </div>
            </div>
          </header>
        )}

        {/* Stream Viewer */}
        <main className={`${isFullscreen ? 'h-screen' : 'flex-1'} flex items-center justify-center p-4`}>
          <div className="relative max-w-full max-h-full">
            
            {/* Stream Display */}
            {stream.status === 'active' ? (
              <div className="relative bg-black rounded-lg overflow-hidden shadow-2xl">
                {streamError ? (
                  <div className="flex items-center justify-center min-h-[400px] min-w-[600px] text-white bg-gray-800">
                    <div className="text-center">
                      <div className="text-4xl mb-2">⚠️</div>
                      <p>Stream temporarily unavailable</p>
                      <p className="text-sm text-gray-400 mt-1">Retrying...</p>
                    </div>
                  </div>
                ) : (
                  <img
                    ref={videoRef}
                    src={`/api/stream/${stream.template_id || stream.id}/mjpeg?t=${Date.now()}`}
                    alt={`${stream.name} live stream`}
                    className={`${isFullscreen ? 'max-h-screen max-w-screen' : 'max-h-[70vh] max-w-full'} object-contain`}
                    onError={handleStreamError}
                    onLoad={() => setStreamError(false)}
                  />
                )}
              </div>
            ) : (
              <div className="flex items-center justify-center min-h-[400px] min-w-[600px] bg-gray-800 rounded-lg text-white">
                <div className="text-center">
                  <div className="text-6xl mb-4">📹</div>
                  <h2 className="text-2xl font-bold mb-2">Stream Offline</h2>
                  <p className="text-gray-400">This stream is currently not active</p>
                </div>
              </div>
            )}

            {/* Stream Controls Overlay */}
            <div className="absolute bottom-4 left-4 right-4 bg-black bg-opacity-50 rounded-lg p-4">
              <div className="flex items-center justify-between text-white">
                <div className="flex items-center space-x-4">
                  <button
                    onClick={loadStream}
                    className="bg-blue-600 hover:bg-blue-700 px-3 py-2 rounded text-sm font-medium flex items-center space-x-2"
                  >
                    <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                    </svg>
                    <span>Refresh</span>
                  </button>
                </div>
                
                <div className="flex items-center space-x-2">
                  <button
                    onClick={toggleFullscreen}
                    className="bg-gray-600 hover:bg-gray-700 px-3 py-2 rounded text-sm font-medium flex items-center space-x-2"
                  >
                    <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d={
                        isFullscreen 
                          ? "M6 10a2 2 0 002-2V6a2 2 0 00-2-2H4a2 2 0 00-2 2v2a2 2 0 002 2h2zM14 10a2 2 0 002-2V6a2 2 0 00-2-2h-2a2 2 0 00-2 2v2a2 2 0 002 2h2zM6 20a2 2 0 002-2v-2a2 2 0 00-2-2H4a2 2 0 00-2 2v2a2 2 0 002 2h2zM14 20a2 2 0 002-2v-2a2 2 0 00-2-2h-2a2 2 0 00-2 2v2a2 2 0 002 2h2z"
                          : "M4 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2V6zM14 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2V6zM4 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2v-2zM14 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2v-2z"
                      } />
                    </svg>
                    <span>{isFullscreen ? 'Exit Fullscreen' : 'Fullscreen'}</span>
                  </button>
                </div>
              </div>
            </div>
          </div>
        </main>

        {/* Stream Info Panel - hidden in fullscreen */}
        {!isFullscreen && (
          <div className="bg-gray-800 border-t border-gray-700 p-4">
            <div className="max-w-7xl mx-auto">
              <div className="grid grid-cols-1 md:grid-cols-3 gap-4 text-sm">
                <div>
                  <span className="text-gray-400">Stream ID:</span>
                  <span className="text-white ml-2">{stream.id}</span>
                </div>
                {stream.template_id && (
                  <div>
                    <span className="text-gray-400">Template ID:</span>
                    <span className="text-white ml-2">{stream.template_id}</span>
                  </div>
                )}
                <div>
                  <span className="text-gray-400">Status:</span>
                  <span className={`ml-2 ${stream.status === 'active' ? 'text-green-400' : 'text-gray-400'}`}>
                    {stream.status}
                  </span>
                </div>
              </div>
            </div>
          </div>
        )}
      </div>
    </ProtectedRoute>
  );
}