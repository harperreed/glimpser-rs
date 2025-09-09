// Stream form dynamic field management
function showConfigFields(streamType) {
    // Hide all config sections
    const sections = document.querySelectorAll('.config-section');
    sections.forEach(section => {
        section.classList.add('hidden');
    });

    // Show the selected config section
    if (streamType) {
        const configSection = document.getElementById(streamType + '-config');
        if (configSection) {
            configSection.classList.remove('hidden');
        }
    }
}

// Initialize form on page load
document.addEventListener('DOMContentLoaded', function() {
    const selectElement = document.getElementById('config_kind');
    if (selectElement) {
        const streamType = selectElement.value;
        showConfigFields(streamType);

        // Add event listener for changes
        selectElement.addEventListener('change', function() {
            showConfigFields(this.value);
        });
    }
});
