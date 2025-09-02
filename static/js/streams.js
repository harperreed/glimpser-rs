// ABOUTME: Stream viewer functionality for live video feeds
// ABOUTME: Handles stream display, filtering, and real-time updates

// Global event handlers to prevent memory leaks
let streamActionsHandler = null;
let modalClickHandler = null;

document.addEventListener('DOMContentLoaded', function() {
    if (!requireAuth()) return;

    setupLogout();
    updateNavigation();
    setupStreamControls();
    setupModalEventListeners();
    loadStreams();

    // Auto-refresh every 10 seconds
    setInterval(loadStreams, 10000);
});

function setupStreamControls() {
    const refreshBtn = document.getElementById('refresh-streams-btn');
    const filterSelect = document.getElementById('stream-filter');

    if (refreshBtn) {
        refreshBtn.addEventListener('click', loadStreams);
    }

    if (filterSelect) {
        filterSelect.addEventListener('change', filterStreams);
    }
}

async function loadStreams() {
    const streamsGrid = document.getElementById('streams-grid');
    const noStreams = document.getElementById('no-streams');

    if (!streamsGrid) return;

    // Show loading
    streamsGrid.innerHTML = `
        <div class="loading-card">
            <div class="spinner"></div>
            <p>Loading streams...</p>
        </div>
    `;

    if (noStreams) {
        noStreams.classList.add('hidden');
    }

    try {
        // Try to load streams from API
        let streams = [];

        try {
            streams = await authManager.apiRequest('/streams');
            if (!Array.isArray(streams)) {
                streams = [];
            }
        } catch (error) {
            console.error('Failed to load streams from API:', error);
            streams = [];
        }

        if (streams.length === 0) {
            streamsGrid.innerHTML = '';
            if (noStreams) {
                noStreams.classList.remove('hidden');
            }
            return;
        }

        displayStreams(streams);

    } catch (error) {
        console.error('Error loading streams:', error);
        streamsGrid.innerHTML = `
            <div class="loading-card">
                <p class="error">Failed to load streams: ${error.message}</p>
            </div>
        `;
    }
}

function displayStreams(streams) {
    const streamsGrid = document.getElementById('streams-grid');

    streamsGrid.innerHTML = streams.map(stream => {
        const statusClass = stream.status === 'active' ? 'online' : 'offline';
        const lastSeen = stream.last_frame_at ? formatTimeAgo(new Date(stream.last_frame_at)) : 'Never';
        const templateId = stream.template_id || stream.id;

        return `
            <div class="stream-card" data-stream-id="${stream.id}" data-template-id="${templateId}" data-status="${stream.status}">
                <div class="stream-preview" data-action="open-modal" data-stream-id="${stream.id}">
                    ${stream.status === 'active' ?
                        `<img src="/api/stream/${stream.id}/thumbnail" alt="${escapeHtml(stream.name)}" class="stream-thumbnail">`
                        : '<span>📹 Offline</span>'
                    }
                </div>
                <div class="stream-info">
                    <h3>${escapeHtml(stream.name)}</h3>
                    <div class="stream-meta">
                        <span class="status-indicator ${statusClass}">
                            ${stream.status === 'active' ? '● Live' : '● Offline'}
                        </span>
                        <span class="last-seen">Last seen: ${lastSeen}</span>
                    </div>
                    <div class="stream-details">
                        <small>${stream.resolution || 'Unknown'} @ ${stream.fps || 0}fps</small>
                    </div>
                    <div class="stream-controls">
                        ${stream.status === 'active' ?
                            `<button data-action="stop-stream" data-template-id="${templateId}" class="btn-danger btn-small">Stop</button>` :
                            `<button data-action="start-stream" data-template-id="${templateId}" class="btn-primary btn-small">Start</button>`
                        }
                    </div>
                </div>
            </div>
        `;
    }).join('');

    // Add event listeners after creating the HTML
    setupStreamEventListeners();
}

function filterStreams() {
    const filter = document.getElementById('stream-filter').value;
    const streamCards = document.querySelectorAll('.stream-card');

    streamCards.forEach(card => {
        const status = card.dataset.status;
        let show = true;

        switch (filter) {
            case 'active':
                show = status === 'active';
                break;
            case 'inactive':
                show = status !== 'active';
                break;
            default:
                show = true;
        }

        card.style.display = show ? 'block' : 'none';
    });
}

async function openStreamModal(streamId) {
    const modal = document.getElementById('stream-modal');
    const modalTitle = document.getElementById('stream-modal-title');
    const modalImage = document.getElementById('stream-image');
    const modalPlaceholder = document.getElementById('stream-placeholder');
    const modalStatus = document.getElementById('modal-stream-status');
    const modalSource = document.getElementById('modal-stream-source');
    const modalResolution = document.getElementById('modal-stream-resolution');
    const modalFps = document.getElementById('modal-stream-fps');

    if (!modal) return;

    try {
        // Try to get stream details
        let stream;
        try {
            stream = await authManager.apiRequest(`/stream/${streamId}`);
        } catch (error) {
            console.error('Failed to load stream details:', error);
            showError('Failed to load stream details');
            return;
        }

        if (!stream) {
            showError('Stream not found');
            return;
        }

        // Update modal content
        if (modalTitle) {
            modalTitle.textContent = stream.name;
        }

        if (modalStatus) {
            modalStatus.innerHTML = `<span class="status-indicator ${stream.status === 'active' ? 'online' : 'offline'}">${stream.status}</span>`;
        }

        if (modalSource) {
            modalSource.textContent = stream.source;
        }

        if (modalResolution) {
            modalResolution.textContent = stream.resolution || 'Unknown';
        }

        if (modalFps) {
            modalFps.textContent = stream.fps ? `${stream.fps} fps` : 'Unknown';
        }

        // Handle stream display
        if (stream.status === 'active') {
            if (modalImage && modalPlaceholder) {
                modalImage.src = `/api/stream/${streamId}/live`;
                modalImage.onerror = () => {
                    modalImage.classList.add('hidden');
                    modalPlaceholder.classList.remove('hidden');
                    modalPlaceholder.innerHTML = '<span>Stream unavailable</span>';
                };
                modalImage.onload = () => {
                    modalImage.classList.remove('hidden');
                    modalPlaceholder.classList.add('hidden');
                };
                modalImage.classList.remove('hidden');
                modalPlaceholder.classList.add('hidden');
            }
        } else {
            if (modalImage && modalPlaceholder) {
                modalImage.classList.add('hidden');
                modalPlaceholder.classList.remove('hidden');
                modalPlaceholder.innerHTML = '<span>Stream offline</span>';
            }
        }

        // Show modal
        modal.classList.remove('hidden');
        document.body.style.overflow = 'hidden';

    } catch (error) {
        console.error('Error opening stream modal:', error);
        showError('Failed to load stream details');
    }
}

function closeModal(modalId) {
    const modal = document.getElementById(modalId);
    if (modal) {
        modal.classList.add('hidden');
        document.body.style.overflow = 'auto';

        // Stop any ongoing stream
        const streamImage = document.getElementById('stream-image');
        if (streamImage) {
            streamImage.src = '';
            streamImage.classList.add('hidden');
        }
    }
}


// Utility functions
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

function showSuccess(message) {
    // Create or update a success message element
    let successElement = document.getElementById('success-message');
    if (!successElement) {
        successElement = document.createElement('div');
        successElement.id = 'success-message';
        successElement.className = 'success-message hidden';
        document.querySelector('.main-content').appendChild(successElement);
    }

    successElement.textContent = message;
    successElement.classList.remove('hidden');
    setTimeout(() => {
        successElement.classList.add('hidden');
    }, 3000);
}

function setupStreamEventListeners() {
    // Handle all stream actions using event delegation
    const streamsGrid = document.getElementById('streams-grid');
    if (!streamsGrid) return;

    // Remove existing listener if it exists
    if (streamActionsHandler) {
        streamsGrid.removeEventListener('click', streamActionsHandler);
    }

    // Create new handler function and store reference
    streamActionsHandler = function(event) {
        handleStreamActions(event);
    };

    // Add event listener with stored reference
    streamsGrid.addEventListener('click', streamActionsHandler);

    // Handle image errors with proper cleanup
    const thumbnails = streamsGrid.querySelectorAll('.stream-thumbnail');
    thumbnails.forEach(img => {
        // Remove any existing error handlers first
        img.removeEventListener('error', handleImageError);
        // Add new handler
        img.addEventListener('error', handleImageError);
    });
}

function handleImageError() {
    this.style.display = 'none';
    this.parentElement.innerHTML = '<span>📹 No Preview</span>';
}

function setupModalEventListeners() {
    // Remove existing listener if it exists to prevent duplicates
    if (modalClickHandler) {
        document.removeEventListener('click', modalClickHandler);
    }

    // Create new handler function and store reference
    modalClickHandler = function(event) {
        const action = event.target.getAttribute('data-action');

        if (action === 'close-modal') {
            const modalId = event.target.getAttribute('data-modal-id');
            if (modalId) {
                closeModal(modalId);
            }
        }

        // Close modal when clicking outside (on modal backdrop)
        if (event.target.classList.contains('modal')) {
            closeModal(event.target.id);
        }
    };

    // Add event listener with stored reference
    document.addEventListener('click', modalClickHandler);
}

function handleStreamActions(event) {
    const action = event.target.getAttribute('data-action');

    if (action === 'open-modal') {
        const streamId = event.target.getAttribute('data-stream-id');
        if (streamId) {
            openStreamModal(streamId);
        }
    } else if (action === 'start-stream') {
        event.stopPropagation();
        const templateId = event.target.getAttribute('data-template-id');
        if (templateId) {
            startStream(templateId, event);
        }
    } else if (action === 'stop-stream') {
        event.stopPropagation();
        const templateId = event.target.getAttribute('data-template-id');
        if (templateId) {
            stopStream(templateId, event);
        }
    }
}

async function startStream(templateId, event) {
    const button = event.target;
    const originalText = button.textContent;

    try {
        button.textContent = 'Starting...';
        button.disabled = true;

        const response = await authManager.apiRequest(`/stream/${templateId}/start`, {
            method: 'POST'
        });

        showSuccess(`Stream started successfully`);

        // Reload streams to update status immediately
        loadStreams();

    } catch (error) {
        console.error('Error starting stream:', error);
        showError(`Failed to start stream: ${error.message}`);

        // Only restore button state on error
        button.textContent = originalText;
        button.disabled = false;
    }
}

async function stopStream(templateId, event) {
    const button = event.target;
    const originalText = button.textContent;

    try {
        button.textContent = 'Stopping...';
        button.disabled = true;

        const response = await authManager.apiRequest(`/stream/${templateId}/stop`, {
            method: 'POST'
        });

        showSuccess(`Stream stopped successfully`);

        // Reload streams to update status immediately
        loadStreams();

    } catch (error) {
        console.error('Error stopping stream:', error);
        showError(`Failed to stop stream: ${error.message}`);

        // Only restore button state on error
        button.textContent = originalText;
        button.disabled = false;
    }
}
