// ABOUTME: Admin panel functionality for user and system management
// ABOUTME: Handles CRUD operations for users, API keys, streams, and system info

document.addEventListener('DOMContentLoaded', function() {
    if (!requireAdmin()) return;

    setupLogout();
    updateNavigation();
    setupTabs();
    setupModals();
    setupStreamTypeHandling();
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

function setupStreamTypeHandling() {
    const streamTypeSelect = document.getElementById('stream-type');
    if (streamTypeSelect) {
        streamTypeSelect.addEventListener('change', function() {
            handleStreamTypeChange(this.value);
        });
    }
}

function handleStreamTypeChange(streamType) {
    const websiteOptions = document.getElementById('website-options');
    const ffmpegOptions = document.getElementById('ffmpeg-options');

    // Hide all sections first
    if (websiteOptions) websiteOptions.classList.add('hidden');
    if (ffmpegOptions) ffmpegOptions.classList.add('hidden');

    // Show relevant sections based on type
    switch (streamType) {
        case 'website':
            if (websiteOptions) websiteOptions.classList.remove('hidden');
            break;
        case 'ffmpeg':
            if (ffmpegOptions) ffmpegOptions.classList.remove('hidden');
            break;
    }
}

function setupModals() {
    const createUserBtn = document.getElementById('create-user-btn');
    const createApiKeyBtn = document.getElementById('create-api-key-btn');
    const createStreamBtn = document.getElementById('create-stream-btn');
    const refreshSystemBtn = document.getElementById('refresh-system-btn');

    if (createUserBtn) {
        createUserBtn.addEventListener('click', () => openModal('user-modal'));
    }

    if (createApiKeyBtn) {
        createApiKeyBtn.addEventListener('click', () => openModal('api-key-modal'));
    }

    if (createStreamBtn) {
        createStreamBtn.addEventListener('click', () => openModal('stream-modal'));
    }

    if (refreshSystemBtn) {
        refreshSystemBtn.addEventListener('click', loadSystemInfo);
    }

    // Setup form submissions
    const userForm = document.getElementById('user-form');
    if (userForm) {
        userForm.addEventListener('submit', handleCreateUser);
    }

    const streamForm = document.getElementById('stream-form');
    if (streamForm) {
        streamForm.addEventListener('submit', handleCreateStream);
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
        case 'streams':
            await loadStreams();
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

async function loadStreams() {
    const tbody = document.getElementById('streams-tbody');
    if (!tbody) return;

    tbody.innerHTML = '<tr><td colspan="4" class="loading">Loading streams...</td></tr>';

    try {
        const streams = await authManager.apiRequest('/admin/streams');

        if (!streams || streams.length === 0) {
            tbody.innerHTML = '<tr><td colspan="4" class="empty">No streams found</td></tr>';
            return;
        }

        tbody.innerHTML = streams.map(stream => `
            <tr>
                <td>${escapeHtml(stream.name)}</td>
                <td>${escapeHtml(stream.type || 'Unknown')}</td>
                <td>${formatDate(stream.created_at)}</td>
                <td>
                    <button data-edit-stream="${stream.id}" class="btn-secondary">Edit</button>
                    <button data-delete-stream="${stream.id}" class="btn-danger">Delete</button>
                </td>
            </tr>
        `).join('');

        // Add event listeners for stream actions
        tbody.querySelectorAll('[data-edit-stream]').forEach(btn => {
            btn.addEventListener('click', (e) => {
                editStream(e.target.dataset.editStream);
            });
        });
        tbody.querySelectorAll('[data-delete-stream]').forEach(btn => {
            btn.addEventListener('click', (e) => {
                deleteStream(e.target.dataset.deleteStream);
            });
        });

    } catch (error) {
        console.error('Error loading streams:', error);
        tbody.innerHTML = '<tr><td colspan="4" class="error">Failed to load streams</td></tr>';
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

async function handleCreateStream(e) {
    e.preventDefault();

    const form = document.getElementById('stream-form');
    const editingId = form.dataset.editingId;
    const isEditing = !!editingId;

    const name = document.getElementById('stream-name').value;
    const description = document.getElementById('stream-description').value;
    const type = document.getElementById('stream-type').value;
    const url = document.getElementById('stream-url').value;
    const isDefault = document.getElementById('stream-default').checked;

    // Build the config object based on stream type
    let config;
    switch (type) {
        case 'website':
            const width = parseInt(document.getElementById('stream-width').value) || 1920;
            const height = parseInt(document.getElementById('stream-height').value) || 1080;
            const elementSelector = document.getElementById('stream-element-selector').value.trim();
            const headless = document.getElementById('stream-headless').checked;

            config = {
                kind: 'website',
                url: url,
                headless: headless,
                stealth: false,
                width: width,
                height: height
            };

            // Add element selector if provided
            if (elementSelector) {
                config.element_selector = elementSelector;
            }
            break;
        case 'ffmpeg':
            const hardwareAccel = document.getElementById('stream-hardware-accel').value;
            config = {
                kind: 'ffmpeg',
                source_url: url,
                reconnect: true
            };
            if (hardwareAccel && hardwareAccel !== 'none') {
                config.hardware_acceleration = hardwareAccel;
            }
            break;
        case 'yt':
            config = {
                kind: 'yt',
                url: url,
                format: 'best',
                is_live: false
            };
            break;
        case 'file':
            config = {
                kind: 'file',
                file_path: url
            };
            break;
        default:
            showError('Unknown stream type: ' + type);
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
            isEditing ? `/api/streams/${editingId}` : '/api/streams',
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

        closeModal('stream-modal');
        resetStreamForm();
        await loadStreams();
        showSuccess(isEditing ? 'Stream updated successfully' : 'Stream created successfully');

    } catch (error) {
        showError(`Failed to ${isEditing ? 'update' : 'create'} stream: ` + error.message);
    }
}

function resetStreamForm() {
    const form = document.getElementById('stream-form');
    form.reset();

    // Clear editing state
    delete form.dataset.editingId;

    // Reset form title and button text to default
    const modalTitle = document.querySelector('#stream-modal h3');
    const submitButton = document.querySelector('#stream-form button[type="submit"]');
    modalTitle.textContent = 'Create Stream';
    submitButton.textContent = 'Create Stream';

    // Reset to default values
    document.getElementById('stream-width').value = 1920;
    document.getElementById('stream-height').value = 1080;
    document.getElementById('stream-headless').checked = true;
    document.getElementById('stream-hardware-accel').value = 'none';

    // Hide all optional sections
    handleStreamTypeChange('');
}

function openModal(modalId) {
    const modal = document.getElementById(modalId);
    if (modal) {
        // Reset stream form if opening stream modal for creation
        if (modalId === 'stream-modal' && !document.getElementById('stream-form').dataset.editingId) {
            resetStreamForm();
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

async function editStream(streamId) {
    try {
        // Fetch the stream data
        const response = await fetch(`/api/streams/${streamId}`, {
            headers: authManager.getAuthHeaders()
        });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);
        const stream = await response.json();

        // Populate the form with existing stream data
        document.getElementById('stream-name').value = stream.name || '';
        document.getElementById('stream-description').value = stream.description || '';
        document.getElementById('stream-default').checked = stream.is_default || false;

        // Parse the config to determine type and URL
        let config;
        try {
            config = typeof stream.config === 'string' ? JSON.parse(stream.config) : stream.config;
        } catch (e) {
            console.error('Error parsing stream config:', e);
            config = {};
        }

        // Set the type and URL based on config
        const typeSelect = document.getElementById('stream-type');
        const urlInput = document.getElementById('stream-url');

        if (config.kind === 'website') {
            typeSelect.value = 'website';
            urlInput.value = config.url || '';

            // Populate website-specific fields
            document.getElementById('stream-width').value = config.width || 1920;
            document.getElementById('stream-height').value = config.height || 1080;
            document.getElementById('stream-element-selector').value = config.element_selector || '';
            document.getElementById('stream-headless').checked = config.headless !== false;
        } else if (config.kind === 'ffmpeg') {
            typeSelect.value = 'ffmpeg';
            urlInput.value = config.source_url || config.rtsp_url || config.url || '';

            // Populate ffmpeg-specific fields
            document.getElementById('stream-hardware-accel').value = config.hardware_acceleration || 'none';
        } else if (config.kind === 'file') {
            typeSelect.value = 'file';
            urlInput.value = config.file_path || config.path || '';
        } else if (config.kind === 'yt' || config.kind === 'youtube') {
            typeSelect.value = 'yt';
            urlInput.value = config.url || '';
        } else {
            // Default to website if unknown
            typeSelect.value = 'website';
            urlInput.value = '';

            // Set default values for website fields
            document.getElementById('stream-width').value = 1920;
            document.getElementById('stream-height').value = 1080;
            document.getElementById('stream-element-selector').value = '';
            document.getElementById('stream-headless').checked = true;
        }

        // Show/hide appropriate sections based on selected type
        handleStreamTypeChange(typeSelect.value);

        // Update form title and button text for editing
        const modalTitle = document.querySelector('#stream-modal h3');
        const submitButton = document.querySelector('#stream-form button[type="submit"]');
        modalTitle.textContent = 'Edit Stream';
        submitButton.textContent = 'Update Stream';

        // Store the stream ID in the form for later use
        document.getElementById('stream-form').dataset.editingId = streamId;

        // Open the modal
        openModal('stream-modal');

    } catch (error) {
        showError('Failed to load stream: ' + error.message);
    }
}

async function deleteStream(streamId) {
    if (!confirm('Are you sure you want to delete this stream?')) return;

    try {
        const response = await fetch(`/api/streams/${streamId}`, {
            method: 'DELETE',
            headers: authManager.getAuthHeaders()
        });
        if (!response.ok) throw new Error(`HTTP ${response.status}`);

        await loadStreams();
        showSuccess('Stream deleted successfully');

    } catch (error) {
        showError('Failed to delete stream: ' + error.message);
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
