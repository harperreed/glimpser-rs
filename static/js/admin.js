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

    // Handle modal close buttons
    document.querySelectorAll('[data-close-modal]').forEach(btn => {
        btn.addEventListener('click', (e) => {
            closeModal(e.target.dataset.closeModal);
        });
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
        const response = await fetch('/api/admin/users', {
            headers: authManager.getAuthHeaders()
        });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        const users = await response.json();

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
                    <button data-edit-user="${user.id}" class="btn-secondary">Edit</button>
                    <button data-delete-user="${user.id}" class="btn-danger">Delete</button>
                </td>
            </tr>
        `).join('');

        // Add event listeners for user actions
        tbody.querySelectorAll('[data-edit-user]').forEach(btn => {
            btn.addEventListener('click', (e) => {
                editUser(e.target.dataset.editUser);
            });
        });
        tbody.querySelectorAll('[data-delete-user]').forEach(btn => {
            btn.addEventListener('click', (e) => {
                deleteUser(e.target.dataset.deleteUser);
            });
        });

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
        const response = await fetch('/api/admin/api-keys', {
            headers: authManager.getAuthHeaders()
        });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        const apiKeys = await response.json();

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
                    <button data-revoke-key="${key.id}" class="btn-danger">Revoke</button>
                </td>
            </tr>
        `).join('');

        // Add event listeners for API key actions
        tbody.querySelectorAll('[data-revoke-key]').forEach(btn => {
            btn.addEventListener('click', (e) => {
                revokeApiKey(e.target.dataset.revokeKey);
            });
        });

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
        const templates = await authManager.apiRequest('admin/templates');

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
                    <button data-edit-template="${template.id}" class="btn-secondary">Edit</button>
                    <button data-delete-template="${template.id}" class="btn-danger">Delete</button>
                </td>
            </tr>
        `).join('');

        // Add event listeners for template actions
        tbody.querySelectorAll('[data-edit-template]').forEach(btn => {
            btn.addEventListener('click', (e) => {
                editTemplate(e.target.dataset.editTemplate);
            });
        });
        tbody.querySelectorAll('[data-delete-template]').forEach(btn => {
            btn.addEventListener('click', (e) => {
                deleteTemplate(e.target.dataset.deleteTemplate);
            });
        });

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
    const username = document.getElementById('user-username').value;
    const password = document.getElementById('user-password').value;
    const role = document.getElementById('user-role').value;

    try {
        const response = await fetch('/api/admin/users', {
            method: 'POST',
            headers: {
                ...authManager.getAuthHeaders(),
                'Content-Type': 'application/json'
            },
            body: JSON.stringify({ email, username, password, role })
        });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);

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

    const form = document.getElementById('template-form');
    const editingId = form.dataset.editingId;
    const isEditing = !!editingId;

    const name = document.getElementById('template-name').value;
    const description = document.getElementById('template-description').value;
    const type = document.getElementById('template-type').value;
    const url = document.getElementById('template-url').value;
    const isDefault = document.getElementById('template-default').checked;

    // Build the config object based on template type
    let config;
    switch (type) {
        case 'website':
            config = {
                kind: 'website',
                url: url,
                headless: true,
                stealth: false,
                width: 1280,
                height: 720
            };
            break;
        case 'rtsp':
            config = {
                kind: 'rtsp',
                rtsp_url: url,
                reconnect: true
            };
            break;
        case 'file':
            config = {
                kind: 'file',
                file_path: url
            };
            break;
        case 'youtube':
            config = {
                kind: 'yt',
                url: url,
                format: 'best',
                is_live: false
            };
            break;
        default:
            showError('Unknown template type: ' + type);
            return;
    }

    try {
        const requestBody = {
            name,
            description: description || null,
            config,
            is_default: isDefault
        };

        const response = await fetch(
            isEditing ? `/api/templates/${editingId}` : '/api/templates',
            {
                method: isEditing ? 'PUT' : 'POST',
                headers: {
                    ...authManager.getAuthHeaders(),
                    'Content-Type': 'application/json'
                },
                body: JSON.stringify(requestBody)
            }
        );
        if (!response.ok) throw new Error(`HTTP ${response.status}`);

        closeModal('template-modal');
        resetTemplateForm();
        await loadTemplates();
        showSuccess(isEditing ? 'Template updated successfully' : 'Template created successfully');

    } catch (error) {
        showError(`Failed to ${isEditing ? 'update' : 'create'} template: ` + error.message);
    }
}

function resetTemplateForm() {
    const form = document.getElementById('template-form');
    form.reset();

    // Clear editing state
    delete form.dataset.editingId;

    // Reset form title and button text to default
    const modalTitle = document.querySelector('#template-modal h3');
    const submitButton = document.querySelector('#template-form button[type="submit"]');
    modalTitle.textContent = 'Create Template';
    submitButton.textContent = 'Create Template';
}

function openModal(modalId) {
    const modal = document.getElementById(modalId);
    if (modal) {
        // Reset template form if opening template modal for creation
        if (modalId === 'template-modal' && !document.getElementById('template-form').dataset.editingId) {
            resetTemplateForm();
        }

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
        const response = await fetch(`/api/admin/users/${userId}`, {
            method: 'DELETE',
            headers: authManager.getAuthHeaders()
        });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);

        await loadUsers();
        showSuccess('User deleted successfully');

    } catch (error) {
        showError('Failed to delete user: ' + error.message);
    }
}

async function revokeApiKey(keyId) {
    if (!confirm('Are you sure you want to revoke this API key?')) return;

    try {
        const response = await fetch(`/api/admin/api-keys/${keyId}`, {
            method: 'DELETE',
            headers: authManager.getAuthHeaders()
        });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);

        await loadApiKeys();
        showSuccess('API key revoked successfully');

    } catch (error) {
        showError('Failed to revoke API key: ' + error.message);
    }
}

async function editTemplate(templateId) {
    try {
        // Fetch the template data
        const response = await fetch(`/api/templates/${templateId}`, {
            headers: authManager.getAuthHeaders()
        });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        const template = await response.json();

        // Populate the form with existing template data
        document.getElementById('template-name').value = template.name || '';
        document.getElementById('template-description').value = template.description || '';
        document.getElementById('template-default').checked = template.is_default || false;

        // Parse the config to determine type and URL
        let config;
        try {
            config = typeof template.config === 'string' ? JSON.parse(template.config) : template.config;
        } catch (e) {
            console.error('Error parsing template config:', e);
            config = {};
        }

        // Set the type and URL based on config
        const typeSelect = document.getElementById('template-type');
        const urlInput = document.getElementById('template-url');

        if (config.kind === 'website') {
            typeSelect.value = 'website';
            urlInput.value = config.url || '';
        } else if (config.kind === 'rtsp') {
            typeSelect.value = 'rtsp';
            urlInput.value = config.rtsp_url || config.url || '';
        } else if (config.kind === 'file') {
            typeSelect.value = 'file';
            urlInput.value = config.file_path || config.path || '';
        } else if (config.kind === 'yt' || config.kind === 'youtube') {
            typeSelect.value = 'youtube';
            urlInput.value = config.url || '';
        } else {
            // Default to website if unknown
            typeSelect.value = 'website';
            urlInput.value = '';
        }

        // Update form title and button text for editing
        const modalTitle = document.querySelector('#template-modal h3');
        const submitButton = document.querySelector('#template-form button[type="submit"]');
        modalTitle.textContent = 'Edit Template';
        submitButton.textContent = 'Update Template';

        // Store the template ID in the form for later use
        document.getElementById('template-form').dataset.editingId = templateId;

        // Open the modal
        openModal('template-modal');

    } catch (error) {
        showError('Failed to load template: ' + error.message);
    }
}

async function deleteTemplate(templateId) {
    if (!confirm('Are you sure you want to delete this template?')) return;

    try {
        const response = await fetch(`/api/templates/${templateId}`, {
            method: 'DELETE',
            headers: authManager.getAuthHeaders()
        });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);

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
