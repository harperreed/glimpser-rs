// ABOUTME: Admin panel functionality for user and system management
// ABOUTME: Handles CRUD operations for users, API keys, templates, and system info

document.addEventListener('DOMContentLoaded', function() {
    if (!requireAdmin()) return;

    setupLogout();
    updateNavigation();
    setupTabs();
    setupModals();
    loadInitialData();
});

function setupTabs() {
    const tabButtons = document.querySelectorAll('.tab-btn');
    const tabContents = document.querySelectorAll('.tab-content');

    tabButtons.forEach(button => {
        button.addEventListener('click', () => {
            const targetTab = button.dataset.tab;

            // Update active states
            tabButtons.forEach(btn => btn.classList.remove('active'));
            tabContents.forEach(content => content.classList.remove('active'));

            button.classList.add('active');
            document.getElementById(`${targetTab}-tab`).classList.add('active');

            // Load tab data
            loadTabData(targetTab);
        });
    });
}

function setupModals() {
    const createUserBtn = document.getElementById('create-user-btn');
    const createApiKeyBtn = document.getElementById('create-api-key-btn');
    const createTemplateBtn = document.getElementById('create-template-btn');
    const refreshSystemBtn = document.getElementById('refresh-system-btn');

    if (createUserBtn) {
        createUserBtn.addEventListener('click', () => openModal('user-modal'));
    }

    if (createApiKeyBtn) {
        createApiKeyBtn.addEventListener('click', () => openModal('api-key-modal'));
    }

    if (createTemplateBtn) {
        createTemplateBtn.addEventListener('click', () => openModal('template-modal'));
    }

    if (refreshSystemBtn) {
        refreshSystemBtn.addEventListener('click', loadSystemInfo);
    }

    // Setup form submissions
    const userForm = document.getElementById('user-form');
    if (userForm) {
        userForm.addEventListener('submit', handleCreateUser);
    }

    const templateForm = document.getElementById('template-form');
    if (templateForm) {
        templateForm.addEventListener('submit', handleCreateTemplate);
    }

    // Close modals when clicking outside
    document.addEventListener('click', (e) => {
        if (e.target.classList.contains('modal')) {
            closeModal(e.target.id);
        }
    });
}

async function loadInitialData() {
    await loadTabData('users'); // Load the default active tab
}

async function loadTabData(tabName) {
    switch (tabName) {
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
            await loadSystemInfo();
            break;
    }
}

async function loadUsers() {
    const tbody = document.getElementById('users-tbody');
    if (!tbody) return;

    tbody.innerHTML = '<tr><td colspan="4" class="loading">Loading users...</td></tr>';

    try {
        const users = await authManager.apiRequest('/admin/users');

        if (!users || users.length === 0) {
            tbody.innerHTML = '<tr><td colspan="4" class="empty">No users found</td></tr>';
            return;
        }

        tbody.innerHTML = users.map(user => `
            <tr>
                <td>${escapeHtml(user.email)}</td>
                <td><span class="badge ${user.role}">${user.role}</span></td>
                <td>${formatDate(user.created_at)}</td>
                <td>
                    <button onclick="editUser('${user.id}')" class="btn-secondary">Edit</button>
                    <button onclick="deleteUser('${user.id}')" class="btn-danger">Delete</button>
                </td>
            </tr>
        `).join('');

    } catch (error) {
        console.error('Error loading users:', error);
        tbody.innerHTML = '<tr><td colspan="4" class="error">Failed to load users</td></tr>';
    }
}

async function loadApiKeys() {
    const tbody = document.getElementById('api-keys-tbody');
    if (!tbody) return;

    tbody.innerHTML = '<tr><td colspan="5" class="loading">Loading API keys...</td></tr>';

    try {
        const apiKeys = await authManager.apiRequest('/admin/api-keys');

        if (!apiKeys || apiKeys.length === 0) {
            tbody.innerHTML = '<tr><td colspan="5" class="empty">No API keys found</td></tr>';
            return;
        }

        tbody.innerHTML = apiKeys.map(key => `
            <tr>
                <td>${escapeHtml(key.name)}</td>
                <td>${escapeHtml(key.user_email || 'Unknown')}</td>
                <td>${formatDate(key.created_at)}</td>
                <td>${key.last_used_at ? formatDate(key.last_used_at) : 'Never'}</td>
                <td>
                    <button onclick="revokeApiKey('${key.id}')" class="btn-danger">Revoke</button>
                </td>
            </tr>
        `).join('');

    } catch (error) {
        console.error('Error loading API keys:', error);
        tbody.innerHTML = '<tr><td colspan="5" class="error">Failed to load API keys</td></tr>';
    }
}

async function loadTemplates() {
    const tbody = document.getElementById('templates-tbody');
    if (!tbody) return;

    tbody.innerHTML = '<tr><td colspan="4" class="loading">Loading templates...</td></tr>';

    try {
        const templates = await authManager.apiRequest('/api/templates');

        if (!templates || templates.length === 0) {
            tbody.innerHTML = '<tr><td colspan="4" class="empty">No templates found</td></tr>';
            return;
        }

        tbody.innerHTML = templates.map(template => `
            <tr>
                <td>${escapeHtml(template.name)}</td>
                <td>${escapeHtml(template.type || 'Unknown')}</td>
                <td>${formatDate(template.created_at)}</td>
                <td>
                    <button onclick="editTemplate('${template.id}')" class="btn-secondary">Edit</button>
                    <button onclick="deleteTemplate('${template.id}')" class="btn-danger">Delete</button>
                </td>
            </tr>
        `).join('');

    } catch (error) {
        console.error('Error loading templates:', error);
        tbody.innerHTML = '<tr><td colspan="4" class="error">Failed to load templates</td></tr>';
    }
}

async function loadSystemInfo() {
    const dbInfo = document.getElementById('db-info');
    const configInfo = document.getElementById('config-info');
    const versionInfo = document.getElementById('version-info');

    // Database info
    if (dbInfo) {
        dbInfo.innerHTML = '<div class="loading">Loading database info...</div>';
        try {
            const health = await authManager.apiRequest('/health');
            dbInfo.innerHTML = `
                <div class="info-item">
                    <label>Status:</label>
                    <span class="status-indicator ${health.status === 'healthy' ? 'online' : 'offline'}">
                        ${health.status || 'Unknown'}
                    </span>
                </div>
                <div class="info-item">
                    <label>Database:</label>
                    <span>${health.database || 'Connected'}</span>
                </div>
            `;
        } catch (error) {
            dbInfo.innerHTML = '<div class="error">Failed to load database info</div>';
        }
    }

    // Configuration info
    if (configInfo) {
        configInfo.innerHTML = `
            <div class="info-item">
                <label>Environment:</label>
                <span>Production</span>
            </div>
            <div class="info-item">
                <label>API Version:</label>
                <span>v1</span>
            </div>
            <div class="info-item">
                <label>Features:</label>
                <span>Rate Limiting, Authentication, RBAC</span>
            </div>
        `;
    }

    // Version info
    if (versionInfo) {
        versionInfo.innerHTML = `
            <div class="info-item">
                <label>Application:</label>
                <span>Glimpser v1.0.0</span>
            </div>
            <div class="info-item">
                <label>Build:</label>
                <span>${new Date().toISOString().split('T')[0]}</span>
            </div>
            <div class="info-item">
                <label>Runtime:</label>
                <span>Rust/Tokio</span>
            </div>
        `;
    }
}

async function handleCreateUser(e) {
    e.preventDefault();

    const email = document.getElementById('user-email').value;
    const password = document.getElementById('user-password').value;
    const role = document.getElementById('user-role').value;

    try {
        await authManager.apiRequest('/admin/users', {
            method: 'POST',
            body: JSON.stringify({ email, password, role })
        });

        closeModal('user-modal');
        document.getElementById('user-form').reset();
        await loadUsers();
        showSuccess('User created successfully');

    } catch (error) {
        showError('Failed to create user: ' + error.message);
    }
}

async function handleCreateTemplate(e) {
    e.preventDefault();

    const name = document.getElementById('template-name').value;
    const description = document.getElementById('template-description').value;
    const type = document.getElementById('template-type').value;
    const url = document.getElementById('template-url').value;
    const isDefault = document.getElementById('template-default').checked;

    try {
        await authManager.apiRequest('/api/templates', {
            method: 'POST',
            body: JSON.stringify({
                name,
                description,
                source_type: type,
                source_url: url,
                is_default: isDefault
            })
        });

        closeModal('template-modal');
        document.getElementById('template-form').reset();
        await loadTemplates();
        showSuccess('Template created successfully');

    } catch (error) {
        showError('Failed to create template: ' + error.message);
    }
}

function openModal(modalId) {
    const modal = document.getElementById(modalId);
    if (modal) {
        modal.classList.remove('hidden');
        document.body.style.overflow = 'hidden';
    }
}

function closeModal(modalId) {
    const modal = document.getElementById(modalId);
    if (modal) {
        modal.classList.add('hidden');
        document.body.style.overflow = 'auto';
    }
}

// Action handlers
async function editUser(userId) {
    // TODO: Implement user editing
    console.log('Edit user:', userId);
}

async function deleteUser(userId) {
    if (!confirm('Are you sure you want to delete this user?')) return;

    try {
        await authManager.apiRequest(`/admin/users/${userId}`, {
            method: 'DELETE'
        });

        await loadUsers();
        showSuccess('User deleted successfully');

    } catch (error) {
        showError('Failed to delete user: ' + error.message);
    }
}

async function revokeApiKey(keyId) {
    if (!confirm('Are you sure you want to revoke this API key?')) return;

    try {
        await authManager.apiRequest(`/admin/api-keys/${keyId}`, {
            method: 'DELETE'
        });

        await loadApiKeys();
        showSuccess('API key revoked successfully');

    } catch (error) {
        showError('Failed to revoke API key: ' + error.message);
    }
}

async function editTemplate(templateId) {
    // TODO: Implement template editing
    console.log('Edit template:', templateId);
}

async function deleteTemplate(templateId) {
    if (!confirm('Are you sure you want to delete this template?')) return;

    try {
        await authManager.apiRequest(`/api/templates/${templateId}`, {
            method: 'DELETE'
        });

        await loadTemplates();
        showSuccess('Template deleted successfully');

    } catch (error) {
        showError('Failed to delete template: ' + error.message);
    }
}

// Utility functions
function formatDate(dateString) {
    if (!dateString) return 'N/A';
    return new Date(dateString).toLocaleDateString();
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

function showSuccess(message) {
    // TODO: Implement success message display
    console.log('Success:', message);
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
