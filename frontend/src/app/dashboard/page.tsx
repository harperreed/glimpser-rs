//! ABOUTME: Dashboard page showing user overview and navigation
//! ABOUTME: Protected route requiring authentication

'use client';

import { ProtectedRoute } from '@/components/ProtectedRoute';
import { Navigation } from '@/components/Navigation';
import { useAuth } from '@/contexts/auth';
import { useRouter } from 'next/navigation';
import { useState, useEffect, useCallback } from 'react';
import { apiClient } from '@/lib/api';

interface SystemStats {
  apiStatus: 'online' | 'offline' | 'warning';
  streamsCount: number;
  alertsCount: number;
  systemHealth: 'healthy' | 'warning' | 'error';
}

interface Activity {
  type: 'alert' | 'system';
  message: string;
  timestamp: string;
}

export default function DashboardPage() {
  const { user } = useAuth();
  const router = useRouter();
  const [stats, setStats] = useState<SystemStats>({
    apiStatus: 'offline',
    streamsCount: 0,
    alertsCount: 0,
    systemHealth: 'error'
  });
  const [activities, setActivities] = useState<Activity[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);


  const loadSystemStats = useCallback(async () => {
    try {
      setError(null);

      // Check API health
      let apiStatus: 'online' | 'offline' = 'offline';
      let systemHealth: 'healthy' | 'warning' | 'error' = 'error';

      try {
        const health = await apiClient.health();
        apiStatus = 'online';
        // Safely check for health status property
        if (typeof health === 'object' && health && 'status' in health) {
          systemHealth = (health as { status: string }).status === 'healthy' ? 'healthy' : 'warning';
        } else {
          systemHealth = 'warning';
        }
      } catch {
        // Health check failed
      }

      // Load streams count
      let streamsCount = 0;
      try {
        const streams = await apiClient.streams();
        streamsCount = Array.isArray(streams) ? streams.length : 0;
      } catch {
        // Failed to load streams count
      }

      // Load alerts count (placeholder - endpoint might not exist)
      let alertsCount = 0;
      try {
        // This endpoint might not exist yet, so we handle gracefully
        const alerts = await apiClient.alerts();
        if (alerts && Array.isArray(alerts)) {
          alertsCount = alerts.length;
        }
      } catch {
        // Alerts endpoint not available
      }

      setStats({
        apiStatus,
        streamsCount,
        alertsCount,
        systemHealth
      });
    } catch (error) {
      console.error('Error loading system stats:', error);
      setError('Failed to load system statistics');
    }
  }, []);

  const loadRecentActivity = useCallback(async () => {
    try {
      const activities: Activity[] = [];

      // Try to load alerts (placeholder)
      try {
        const alerts = await apiClient.alerts();
        if (alerts && Array.isArray(alerts)) {
          alerts.slice(0, 5).forEach((alert: { message?: string; created_at?: string }) => {
            activities.push({
              type: 'alert',
              message: `Alert: ${alert.message || 'New alert received'}`,
              timestamp: alert.created_at || new Date().toISOString()
            });
          });
        }
      } catch {
        // No alerts available
      }

      // Add system activity
      activities.push({
        type: 'system',
        message: 'System monitoring active',
        timestamp: new Date().toISOString()
      });

      // Sort by timestamp (newest first)
      activities.sort((a, b) => new Date(b.timestamp).getTime() - new Date(a.timestamp).getTime());

      setActivities(activities.slice(0, 10));
    } catch (error) {
      console.error('Error loading recent activity:', error);
    }
  }, []);

  const refreshDashboard = useCallback(async () => {
    setIsRefreshing(true);
    try {
      await Promise.all([
        loadSystemStats(),
        loadRecentActivity()
      ]);
    } catch (error) {
      console.error('Error refreshing dashboard:', error);
      setError('Failed to refresh dashboard');
    } finally {
      setIsRefreshing(false);
    }
  }, [loadSystemStats, loadRecentActivity]);

  // Load data on mount
  useEffect(() => {
    let isMounted = true;
    const loadData = async () => {
      setIsLoading(true);
      await refreshDashboard();
      if (isMounted) {
        setIsLoading(false);
      }
    };
    loadData();
    return () => {
      isMounted = false;
    };
  }, [refreshDashboard]);

  // Auto-refresh every 30 seconds
  useEffect(() => {
    const interval = setInterval(refreshDashboard, 30000);
    return () => clearInterval(interval);
  }, [refreshDashboard]);

  const formatTimeAgo = (date: Date) => {
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / (1000 * 60));
    const diffHours = Math.floor(diffMins / 60);
    const diffDays = Math.floor(diffHours / 24);

    if (diffMins < 1) return 'Just now';
    if (diffMins < 60) return `${diffMins}m ago`;
    if (diffHours < 24) return `${diffHours}h ago`;
    if (diffDays < 7) return `${diffDays}d ago`;

    return date.toLocaleDateString();
  };

  const getStatusColor = (status: string) => {
    switch (status) {
      case 'online':
      case 'healthy':
        return 'text-green-600';
      case 'warning':
        return 'text-yellow-600';
      case 'offline':
      case 'error':
      default:
        return 'text-red-600';
    }
  };

  const getStatusBg = (status: string) => {
    switch (status) {
      case 'online':
      case 'healthy':
        return 'bg-green-500';
      case 'warning':
        return 'bg-yellow-500';
      case 'offline':
      case 'error':
      default:
        return 'bg-red-500';
    }
  };

  return (
    <ProtectedRoute>
      <div className="min-h-screen bg-gray-100">
        {/* Header */}
        <Navigation />

        <header className="bg-white shadow">
          <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
            <div className="flex justify-between items-center py-4">
              <div className="flex items-center">
                <h2 className="text-2xl font-bold text-gray-900">
                  Dashboard
                </h2>
                {isLoading && (
                  <div className="ml-4 w-6 h-6 border-2 border-transparent border-t-blue-600 rounded-full animate-spin"></div>
                )}
              </div>
              <button
                onClick={refreshDashboard}
                disabled={isRefreshing}
                className="bg-blue-600 hover:bg-blue-700 disabled:bg-blue-400 text-white px-4 py-2 rounded-md text-sm font-medium flex items-center space-x-2"
              >
                {isRefreshing ? (
                  <>
                    <div className="w-4 h-4 border-2 border-transparent border-t-white rounded-full animate-spin"></div>
                    <span>Refreshing...</span>
                  </>
                ) : (
                  <>
                    <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
                    </svg>
                    <span>Refresh</span>
                  </>
                )}
              </button>
            </div>
          </div>
        </header>

        {/* Error Message */}
        {error && (
          <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 pt-4">
            <div className="bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-4">
              {error}
            </div>
          </div>
        )}

        {/* Main Content */}
        <main className="max-w-7xl mx-auto py-6 sm:px-6 lg:px-8">
          <div className="px-4 py-6 sm:px-0">
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">

              {/* Streams Card */}
              <div className="bg-white overflow-hidden shadow rounded-lg">
                <div className="p-5">
                  <div className="flex items-center">
                    <div className="flex-shrink-0">
                      <div className="w-8 h-8 bg-blue-500 rounded-full flex items-center justify-center">
                        <svg className="w-4 h-4 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
                        </svg>
                      </div>
                    </div>
                    <div className="ml-5 w-0 flex-1">
                      <dl>
                        <dt className="text-sm font-medium text-gray-500 truncate">
                          Active Streams
                        </dt>
                        <dd className="text-lg font-medium text-gray-900">
                          {isLoading ? '...' : stats.streamsCount}
                        </dd>
                      </dl>
                    </div>
                  </div>
                </div>
                <div className="bg-gray-50 px-5 py-3">
                  <div className="text-sm">
                    <button
                      onClick={() => router.push('/streams')}
                      className="font-medium text-blue-600 hover:text-blue-500"
                    >
                      View all streams
                    </button>
                  </div>
                </div>
              </div>

              {/* Alerts Card */}
              <div className="bg-white overflow-hidden shadow rounded-lg">
                <div className="p-5">
                  <div className="flex items-center">
                    <div className="flex-shrink-0">
                      <div className="w-8 h-8 bg-yellow-500 rounded-full flex items-center justify-center">
                        <svg className="w-4 h-4 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-2.5L13.732 4c-.77-.833-1.964-.833-2.732 0L4.082 16.5c-.77.833.192 2.5 1.732 2.5z" />
                        </svg>
                      </div>
                    </div>
                    <div className="ml-5 w-0 flex-1">
                      <dl>
                        <dt className="text-sm font-medium text-gray-500 truncate">
                          Recent Alerts
                        </dt>
                        <dd className="text-lg font-medium text-gray-900">
                          {isLoading ? '...' : stats.alertsCount}
                        </dd>
                      </dl>
                    </div>
                  </div>
                </div>
                <div className="bg-gray-50 px-5 py-3">
                  <div className="text-sm">
                    <button className="font-medium text-yellow-600 hover:text-yellow-500">
                      View all alerts
                    </button>
                  </div>
                </div>
              </div>

              {/* API Status Card */}
              <div className="bg-white overflow-hidden shadow rounded-lg">
                <div className="p-5">
                  <div className="flex items-center">
                    <div className="flex-shrink-0">
                      <div className={`w-8 h-8 ${getStatusBg(stats.apiStatus)} rounded-full flex items-center justify-center`}>
                        <svg className="w-4 h-4 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
                        </svg>
                      </div>
                    </div>
                    <div className="ml-5 w-0 flex-1">
                      <dl>
                        <dt className="text-sm font-medium text-gray-500 truncate">
                          API Status
                        </dt>
                        <dd className={`text-lg font-medium ${getStatusColor(stats.apiStatus)}`}>
                          {isLoading ? '...' : stats.apiStatus === 'online' ? 'Online' : 'Offline'}
                        </dd>
                      </dl>
                    </div>
                  </div>
                </div>
                <div className="bg-gray-50 px-5 py-3">
                  <div className="text-sm">
                    <button className={`font-medium ${getStatusColor(stats.apiStatus)} hover:opacity-75`}>
                      View API details
                    </button>
                  </div>
                </div>
              </div>

              {/* System Health Card */}
              <div className="bg-white overflow-hidden shadow rounded-lg">
                <div className="p-5">
                  <div className="flex items-center">
                    <div className="flex-shrink-0">
                      <div className={`w-8 h-8 ${getStatusBg(stats.systemHealth)} rounded-full flex items-center justify-center`}>
                        <svg className="w-4 h-4 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4.318 6.318a4.5 4.5 0 000 6.364L12 20.364l7.682-7.682a4.5 4.5 0 00-6.364-6.364L12 7.636l-1.318-1.318a4.5 4.5 0 00-6.364 0z" />
                        </svg>
                      </div>
                    </div>
                    <div className="ml-5 w-0 flex-1">
                      <dl>
                        <dt className="text-sm font-medium text-gray-500 truncate">
                          System Health
                        </dt>
                        <dd className={`text-lg font-medium ${getStatusColor(stats.systemHealth)}`}>
                          {isLoading ? '...' : stats.systemHealth === 'healthy' ? 'Healthy' : stats.systemHealth === 'warning' ? 'Warning' : 'Error'}
                        </dd>
                      </dl>
                    </div>
                  </div>
                </div>
                <div className="bg-gray-50 px-5 py-3">
                  <div className="text-sm">
                    <button className={`font-medium ${getStatusColor(stats.systemHealth)} hover:opacity-75`}>
                      View health details
                    </button>
                  </div>
                </div>
              </div>

            </div>

            {/* Recent Activity and Quick Actions */}
            <div className="mt-8 grid grid-cols-1 lg:grid-cols-3 gap-6">

              {/* Recent Activity */}
              <div className="lg:col-span-2">
                <div className="bg-white shadow rounded-lg">
                  <div className="px-4 py-5 sm:p-6">
                    <h3 className="text-lg leading-6 font-medium text-gray-900 mb-4">
                      Recent Activity
                    </h3>
                    <div className="space-y-3">
                      {isLoading ? (
                        <div className="flex items-center justify-center py-8">
                          <div className="w-6 h-6 border-2 border-transparent border-t-gray-600 rounded-full animate-spin"></div>
                          <span className="ml-2 text-gray-500">Loading activity...</span>
                        </div>
                      ) : activities.length === 0 ? (
                        <div className="text-center py-8 text-gray-500">
                          No recent activity
                        </div>
                      ) : (
                        activities.map((activity, index) => (
                          <div key={index} className="flex items-start space-x-3 py-2 border-b border-gray-100 last:border-b-0">
                            <div className={`w-2 h-2 rounded-full mt-2 ${
                              activity.type === 'alert' ? 'bg-yellow-500' : 'bg-blue-500'
                            }`}></div>
                            <div className="flex-1 min-w-0">
                              <p className="text-sm text-gray-900">{activity.message}</p>
                              <p className="text-xs text-gray-500">
                                {formatTimeAgo(new Date(activity.timestamp))}
                              </p>
                            </div>
                          </div>
                        ))
                      )}
                    </div>
                  </div>
                </div>
              </div>

              {/* Quick Actions */}
              <div>
                <div className="bg-white shadow rounded-lg">
                  <div className="px-4 py-5 sm:p-6">
                    <h3 className="text-lg leading-6 font-medium text-gray-900 mb-4">
                      Quick Actions
                    </h3>
                    <div className="space-y-3">
                      <button
                        onClick={() => router.push('/streams')}
                        className="w-full inline-flex items-center px-4 py-2 border border-gray-300 shadow-sm text-sm font-medium rounded-md text-gray-700 bg-white hover:bg-gray-50"
                      >
                        <svg className="w-4 h-4 mr-2" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 10l4.553-2.276A1 1 0 0121 8.618v6.764a1 1 0 01-1.447.894L15 14M5 18h8a2 2 0 002-2V8a2 2 0 00-2-2H5a2 2 0 00-2 2v8a2 2 0 002 2z" />
                        </svg>
                        Manage Streams
                      </button>
                      <button
                        onClick={() => router.push('/admin')}
                        className="w-full inline-flex items-center px-4 py-2 border border-gray-300 shadow-sm text-sm font-medium rounded-md text-gray-700 bg-white hover:bg-gray-50"
                      >
                        <svg className="w-4 h-4 mr-2" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                        </svg>
                        Admin Panel
                      </button>
                      <button className="w-full inline-flex items-center px-4 py-2 border border-gray-300 shadow-sm text-sm font-medium rounded-md text-gray-700 bg-white hover:bg-gray-50">
                        <svg className="w-4 h-4 mr-2" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 6.253v13m0-13C10.832 5.477 9.246 5 7.5 5S4.168 5.477 3 6.253v13C4.168 18.477 5.754 18 7.5 18s3.332.477 4.5 1.253m0-13C13.168 5.477 14.754 5 16.5 5c1.746 0 3.332.477 4.5 1.253v13C19.832 18.477 18.246 18 16.5 18c-1.746 0-3.332.477-4.5 1.253" />
                        </svg>
                        API Documentation
                      </button>
                    </div>
                  </div>
                </div>
              </div>

            </div>
          </div>
        </main>
      </div>
    </ProtectedRoute>
  );
}
