// ABOUTME: Authentication handling and login functionality
// ABOUTME: Manages JWT tokens and user session state

class AuthManager {
    constructor() {
        this.apiBase = '/api';
        this.token = localStorage.getItem('auth_token');
        this.user = (this.token && this.token.trim()) ? this.parseJWT(this.token) : null;
    }

    parseJWT(token) {
        if (!token || typeof token !== 'string' || !token.trim()) {
            return null;
        }
        try {
            const parts = token.split('.');
            if (parts.length !== 3) {
                console.warn('Invalid JWT format: expected 3 parts, got', parts.length);
                return null;
            }
            const base64Url = parts[1];
            const base64 = base64Url.replace(/-/g, '+').replace(/_/g, '/');
            const jsonPayload = decodeURIComponent(atob(base64).split('').map(function(c) {
                return '%' + ('00' + c.charCodeAt(0).toString(16)).slice(-2);
            }).join(''));
            return JSON.parse(jsonPayload);
        } catch (e) {
            console.error('Invalid JWT token:', e);
            return null;
        }
    }

    isAuthenticated() {
        if (!this.token || !this.user) return false;

        // Check if token is expired
        const now = Math.floor(Date.now() / 1000);
        return this.user.exp > now;
    }

    isAdmin() {
        return this.user && this.user.role === 'admin';
    }

    async login(email, password) {
        try {
            const response = await fetch(`${this.apiBase}/auth/login`, {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify({ email, password }),
            });

            if (!response.ok) {
                const error = await response.json();
                throw new Error(error.detail || 'Login failed');
            }

            const data = await response.json();
            this.token = data.access_token;
            this.user = this.parseJWT(this.token);

            localStorage.setItem('auth_token', this.token);
            return data;
        } catch (error) {
            console.error('Login error:', error);
            throw error;
        }
    }

    logout() {
        this.token = null;
        this.user = null;
        localStorage.removeItem('auth_token');
        window.location.href = '/static/index.html';
    }

    getAuthHeaders() {
        return this.token ? {
            'Authorization': `Bearer ${this.token}`,
            'Content-Type': 'application/json'
        } : {
            'Content-Type': 'application/json'
        };
    }

    async apiRequest(endpoint, options = {}) {
        const url = `${this.apiBase}${endpoint}`;
        const config = {
            ...options,
            headers: {
                ...this.getAuthHeaders(),
                ...options.headers
            }
        };

        try {
            const response = await fetch(url, config);

            if (response.status === 401) {
                this.logout();
                return;
            }

            if (!response.ok) {
                const error = await response.json().catch(() => ({ detail: 'Network error' }));
                throw new Error(error.detail || `HTTP ${response.status}`);
            }

            const contentType = response.headers.get('content-type');
            if (contentType && contentType.includes('application/json')) {
                return await response.json();
            }

            return await response.text();
        } catch (error) {
            console.error(`API request failed: ${endpoint}`, error);
            throw error;
        }
    }
}

// Global auth manager instance
const authManager = new AuthManager();

// Login form handling (only for login page)
document.addEventListener('DOMContentLoaded', function() {
    const loginForm = document.getElementById('login-form');

    // Only run login page logic if we're actually on the login page
    if (!loginForm) {
        return; // Not the login page, skip this logic
    }

    // Redirect if already logged in (only from login page)
    if (authManager.isAuthenticated()) {
        window.location.href = '/static/dashboard.html';
        return;
    }
    const loginBtn = document.getElementById('login-btn');
    const errorMessage = document.getElementById('error-message');
    const spinner = loginBtn.querySelector('.spinner');
    const buttonText = loginBtn.querySelector('span');

    function showError(message) {
        errorMessage.textContent = message;
        errorMessage.classList.remove('hidden');
    }

    function hideError() {
        errorMessage.classList.add('hidden');
    }

    function setLoading(loading) {
        loginBtn.disabled = loading;
        if (loading) {
            spinner.classList.remove('hidden');
            buttonText.textContent = 'Signing in...';
        } else {
            spinner.classList.add('hidden');
            buttonText.textContent = 'Login';
        }
    }

    loginForm.addEventListener('submit', async function(e) {
        e.preventDefault();

        const email = document.getElementById('email').value;
        const password = document.getElementById('password').value;

        hideError();
        setLoading(true);

        try {
            await authManager.login(email, password);
            window.location.href = '/static/dashboard.html';
        } catch (error) {
            showError(error.message);
        } finally {
            setLoading(false);
        }
    });
});

// Utility functions for other pages
function requireAuth() {
    if (!authManager.isAuthenticated()) {
        window.location.href = '/static/index.html';
        return false;
    }
    return true;
}

function requireAdmin() {
    if (!authManager.isAuthenticated() || !authManager.isAdmin()) {
        window.location.href = '/static/dashboard.html';
        return false;
    }
    return true;
}

function setupLogout() {
    const logoutBtn = document.getElementById('logout-btn');
    if (logoutBtn) {
        logoutBtn.addEventListener('click', () => {
            authManager.logout();
        });
    }
}

function updateNavigation() {
    const adminLink = document.getElementById('admin-link');
    if (adminLink && authManager.isAdmin()) {
        adminLink.classList.remove('hidden');
    }
}
