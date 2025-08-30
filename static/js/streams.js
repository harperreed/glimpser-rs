// ABOUTME: Stream viewer functionality for live video feeds
// ABOUTME: Handles stream display, filtering, and real-time updates

document.addEventListener('DOMContentLoaded', function() {
    if (!requireAuth()) return;

    setupLogout();
    updateNavigation();
    setupStreamControls();
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
            console.log('Streams API not available, showing placeholder data');
            // Create some placeholder stream data for demonstration
            streams = [
                {
                    id: 'stream-1',
                    name: 'Front Door Camera',
                    source: 'rtsp://192.168.1.100/stream',
                    status: 'active',
                    resolution: '1920x1080',
                    fps: 30,
                    last_frame_at: new Date().toISOString()
                },
                {
                    id: 'stream-2',
                    name: 'Parking Lot',
                    source: 'rtsp://192.168.1.101/stream',
                    status: 'inactive',
                    resolution: '1280x720',
                    fps: 15,
                    last_frame_at: new Date(Date.now() - 300000).toISOString()
                },
                {
                    id: 'stream-3',
                    name: 'Office Lobby',
                    source: 'http://192.168.1.102/mjpeg',
                    status: 'active',
                    resolution: '1920x1080',
                    fps: 25,
                    last_frame_at: new Date().toISOString()
                }
            ];
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

        return `
            <div class="stream-card" data-stream-id="${stream.id}" data-status="${stream.status}">
                <div class="stream-preview" onclick="openStreamModal('${stream.id}')">
                    ${stream.status === 'active' ?
                        `<img src="/api/streams/${stream.id}/thumbnail" alt="${escapeHtml(stream.name)}" onerror="this.style.display='none'; this.parentElement.innerHTML='<span>📹 No Preview</span>'">`
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
                </div>
            </div>
        `;
    }).join('');
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
            stream = await authManager.apiRequest(`/streams/${streamId}`);
        } catch (error) {
            // Fallback to mock data for demonstration
            const mockStreams = {
                'stream-1': {
                    id: 'stream-1',
                    name: 'Front Door Camera',
                    source: 'rtsp://192.168.1.100/stream',
                    status: 'active',
                    resolution: '1920x1080',
                    fps: 30
                },
                'stream-2': {
                    id: 'stream-2',
                    name: 'Parking Lot',
                    source: 'rtsp://192.168.1.101/stream',
                    status: 'inactive',
                    resolution: '1280x720',
                    fps: 15
                },
                'stream-3': {
                    id: 'stream-3',
                    name: 'Office Lobby',
                    source: 'http://192.168.1.102/mjpeg',
                    status: 'active',
                    resolution: '1920x1080',
                    fps: 25
                }
            };
            stream = mockStreams[streamId];
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
                modalImage.src = `/api/streams/${streamId}/live`;
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

// Close modal when clicking outside
document.addEventListener('click', (e) => {
    if (e.target.classList.contains('modal')) {
        closeModal(e.target.id);
    }
});

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
