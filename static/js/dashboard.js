// ABOUTME: Dashboard functionality and system status monitoring
// ABOUTME: Displays real-time system metrics and user information

// Prevent multiple initializations
let dashboardInitialized = false;
let refreshInterval = null;

document.addEventListener('DOMContentLoaded', function() {
    if (dashboardInitialized) {
        console.warn('Dashboard already initialized, skipping duplicate initialization');
        return;
    }

    if (!requireAuth()) return;

    dashboardInitialized = true;

    setupLogout();
    updateNavigation();
    loadUserInfo();
    loadSystemStats();
    loadRecentActivity();

    // Setup refresh button
    const refreshBtn = document.getElementById('refresh-btn');
    if (refreshBtn) {
        refreshBtn.addEventListener('click', refreshDashboard);
    }

    // Clear any existing interval and set new one
    if (refreshInterval) {
        clearInterval(refreshInterval);
    }
    refreshInterval = setInterval(refreshDashboard, 30000);
});

function loadUserInfo() {
    const userName = document.getElementById('user-name');
    const userRole = document.getElementById('user-role');

    if (authManager.user) {
        if (userName) {
            userName.textContent = authManager.user.sub || 'Unknown User';
        }
        if (userRole) {
            userRole.textContent = authManager.user.role || 'user';
            userRole.className = `badge ${authManager.user.role || 'user'}`;
        }
    }
}

// Prevent concurrent loadSystemStats calls
let loadingStats = false;

async function loadSystemStats() {
    if (loadingStats) {
        console.warn('loadSystemStats already running, skipping duplicate call');
        return;
    }
    loadingStats = true;
    const apiStatus = document.getElementById('api-status');
    const streamsCount = document.getElementById('streams-count');
    const alertsCount = document.getElementById('alerts-count');
    const healthStatus = document.getElementById('health-status');

    // Check API status
    try {
        await authManager.apiRequest('/health');
        updateStatusIndicator(apiStatus, 'online', 'API Online');
    } catch (error) {
        updateStatusIndicator(apiStatus, 'offline', 'API Offline');
    }

    // Load streams count
    try {
        // Note: This endpoint might not exist yet, so we'll handle gracefully
        const streams = await authManager.apiRequest('/streams').catch(() => null);
        if (streams && Array.isArray(streams)) {
            streamsCount.textContent = streams.length;
        } else {
            streamsCount.textContent = '0';
        }
    } catch (error) {
        streamsCount.textContent = '0';
    }

    // Load alerts count
    try {
        const alerts = await authManager.apiRequest('/alerts?limit=10').catch(() => null);
        if (alerts && Array.isArray(alerts)) {
            alertsCount.textContent = alerts.length;
        } else {
            alertsCount.textContent = '0';
        }
    } catch (error) {
        alertsCount.textContent = '0';
    }

    // Check system health
    try {
        const health = await authManager.apiRequest('/health');
        if (health && health.status === 'healthy') {
            updateStatusIndicator(healthStatus, 'online', 'System Healthy');
        } else {
            updateStatusIndicator(healthStatus, 'warning', 'System Warning');
        }
    } catch (error) {
        updateStatusIndicator(healthStatus, 'offline', 'System Error');
    } finally {
        loadingStats = false;
    }
}

function updateStatusIndicator(element, status, text) {
    if (!element) return;

    element.innerHTML = `<span class="status-dot ${status}"></span><span>${text}</span>`;
    element.className = `status-indicator ${status}`;
}

async function loadRecentActivity() {
    const activityList = document.getElementById('recent-activity');
    if (!activityList) return;

    try {
        // Try to load recent events/activities
        // This is a placeholder since the exact endpoint structure isn't defined yet
        const activities = [];

        // Try alerts first
        try {
            const alerts = await authManager.apiRequest('/alerts?limit=5');
            if (Array.isArray(alerts)) {
                alerts.forEach(alert => {
                    activities.push({
                        type: 'alert',
                        message: `Alert: ${alert.message || 'New alert received'}`,
                        timestamp: alert.created_at || new Date().toISOString()
                    });
                });
            }
        } catch (e) {
            console.log('No alerts endpoint available');
        }

        // Add some system activities
        activities.push({
            type: 'system',
            message: 'System started successfully',
            timestamp: new Date().toISOString()
        });

        if (activities.length === 0) {
            activityList.innerHTML = '<div class="empty-activity">No recent activity</div>';
            return;
        }

        // Sort by timestamp (newest first)
        activities.sort((a, b) => new Date(b.timestamp) - new Date(a.timestamp));

        activityList.innerHTML = activities.slice(0, 10).map(activity => {
            const timeAgo = formatTimeAgo(new Date(activity.timestamp));
            return `
                <div class="activity-item">
                    <div class="activity-message">${escapeHtml(activity.message)}</div>
                    <div class="activity-time">${timeAgo}</div>
                </div>
            `;
        }).join('');

    } catch (error) {
        console.error('Error loading recent activity:', error);
        activityList.innerHTML = '<div class="error">Failed to load recent activity</div>';
    }
}

function formatTimeAgo(date) {
    const now = new Date();
    const diffMs = now - date;
    const diffMins = Math.floor(diffMs / 60000);
    const diffHours = Math.floor(diffMins / 60);
    const diffDays = Math.floor(diffHours / 24);

    if (diffMins < 1) return 'Just now';
    if (diffMins < 60) return `${diffMins}m ago`;
    if (diffHours < 24) return `${diffHours}h ago`;
    if (diffDays < 7) return `${diffDays}d ago`;

    return date.toLocaleDateString();
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

async function refreshDashboard() {
    const refreshBtn = document.getElementById('refresh-btn');
    const originalText = refreshBtn?.textContent;

    if (refreshBtn) {
        refreshBtn.disabled = true;
        refreshBtn.innerHTML = '<div class="spinner"></div> Refreshing...';
    }

    try {
        await Promise.all([
            loadSystemStats(),
            loadRecentActivity()
        ]);
    } catch (error) {
        console.error('Error refreshing dashboard:', error);
        showError('Failed to refresh dashboard data');
    } finally {
        if (refreshBtn) {
            refreshBtn.disabled = false;
            refreshBtn.textContent = originalText;
        }
    }
}

function showError(message) {
    const errorElement = document.getElementById('error-message');
    if (errorElement) {
        errorElement.textContent = message;
        errorElement.classList.remove('hidden');
        setTimeout(() => {
            errorElement.classList.add('hidden');
        }, 5000);
    }
}
