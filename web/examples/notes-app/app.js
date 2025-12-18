/**
 * EntiDB Notes App
 * 
 * A persistent notes application demonstrating EntiDB's WASM storage.
 */

import * as store from './store.js';

// State
let notes = [];
let activeNoteId = null;
let searchQuery = '';

// DOM Elements
const notesList = document.getElementById('notesList');
const mainContent = document.getElementById('mainContent');
const newNoteBtn = document.getElementById('newNoteBtn');
const searchInput = document.getElementById('searchInput');
const storageDot = document.getElementById('storageDot');
const storageType = document.getElementById('storageType');
const syncStatus = document.getElementById('syncStatus');
const toastContainer = document.getElementById('toastContainer');

/**
 * Show a toast notification.
 */
function showToast(message, type = 'info') {
    const toast = document.createElement('div');
    toast.className = 'toast';
    toast.innerHTML = `
        <span class="status-dot ${type}"></span>
        <span>${message}</span>
    `;
    toastContainer.appendChild(toast);
    
    setTimeout(() => {
        toast.style.opacity = '0';
        toast.style.transform = 'translateX(100%)';
        setTimeout(() => toast.remove(), 200);
    }, 3000);
}

/**
 * Update storage status display.
 */
function updateStorageStatus(info) {
    if (info.persistent) {
        storageDot.className = 'status-dot online';
        storageType.textContent = info.type.toUpperCase();
        syncStatus.textContent = 'Persistent';
    } else {
        storageDot.className = 'status-dot warning';
        storageType.textContent = 'Memory';
        syncStatus.textContent = 'Not Persistent';
    }
}

/**
 * Render the notes list.
 */
function renderNotesList() {
    const filteredNotes = searchQuery 
        ? store.searchNotes(searchQuery)
        : notes;
    
    if (filteredNotes.length === 0) {
        notesList.innerHTML = `
            <div class="empty-state" style="padding: 2rem;">
                <p>${searchQuery ? 'No notes match your search' : 'No notes yet'}</p>
            </div>
        `;
        return;
    }
    
    notesList.innerHTML = filteredNotes.map(note => `
        <div class="note-item ${note.id === activeNoteId ? 'active' : ''}" 
             data-id="${note.id}">
            <div class="note-item-title">${escapeHtml(note.title) || 'Untitled'}</div>
            <div class="note-item-preview">${escapeHtml(note.content.slice(0, 50)) || 'No content'}</div>
        </div>
    `).join('');
    
    // Add click handlers
    notesList.querySelectorAll('.note-item').forEach(item => {
        item.addEventListener('click', () => {
            selectNote(item.dataset.id);
        });
    });
}

/**
 * Render the editor.
 */
function renderEditor(note) {
    if (!note) {
        mainContent.innerHTML = `
            <div class="empty-state">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                    <path d="M9 12h6M12 9v6"/>
                    <rect x="3" y="3" width="18" height="18" rx="2"/>
                </svg>
                <p>Select a note or create a new one</p>
            </div>
        `;
        return;
    }
    
    const updatedDate = new Date(note.updated).toLocaleString();
    const wordCount = note.content.split(/\s+/).filter(w => w).length;
    
    mainContent.innerHTML = `
        <div class="editor-header">
            <input type="text" class="title-input" id="titleInput" 
                   value="${escapeHtml(note.title)}" 
                   placeholder="Note title...">
            <div class="editor-actions">
                <button class="action-btn" id="saveBtn" title="Save">
                    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" 
                         stroke="currentColor" stroke-width="2">
                        <path d="M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z"/>
                        <polyline points="17 21 17 13 7 13 7 21"/>
                        <polyline points="7 3 7 8 15 8"/>
                    </svg>
                </button>
                <button class="action-btn danger" id="deleteBtn" title="Delete">
                    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" 
                         stroke="currentColor" stroke-width="2">
                        <polyline points="3 6 5 6 21 6"/>
                        <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
                    </svg>
                </button>
            </div>
        </div>
        <div class="editor-content">
            <textarea class="content-textarea" id="contentInput" 
                      placeholder="Start writing...">${escapeHtml(note.content)}</textarea>
        </div>
        <div class="editor-footer">
            <span>${wordCount} words</span>
            <span>Last updated: ${updatedDate}</span>
        </div>
    `;
    
    // Bind events
    const titleInput = document.getElementById('titleInput');
    const contentInput = document.getElementById('contentInput');
    const saveBtn = document.getElementById('saveBtn');
    const deleteBtn = document.getElementById('deleteBtn');
    
    let saveTimeout;
    const autoSave = () => {
        clearTimeout(saveTimeout);
        saveTimeout = setTimeout(async () => {
            const updatedNote = await store.saveNote({
                id: note.id,
                title: titleInput.value,
                content: contentInput.value
            });
            notes = notes.map(n => n.id === note.id ? updatedNote : n);
            renderNotesList();
            syncStatus.textContent = 'Saved';
        }, 500);
    };
    
    titleInput.addEventListener('input', autoSave);
    contentInput.addEventListener('input', autoSave);
    
    saveBtn.addEventListener('click', async () => {
        const updatedNote = await store.saveNote({
            id: note.id,
            title: titleInput.value,
            content: contentInput.value
        });
        notes = notes.map(n => n.id === note.id ? updatedNote : n);
        renderNotesList();
        showToast('Note saved', 'online');
    });
    
    deleteBtn.addEventListener('click', async () => {
        if (confirm('Delete this note?')) {
            await store.deleteNote(note.id);
            notes = notes.filter(n => n.id !== note.id);
            activeNoteId = null;
            renderNotesList();
            renderEditor(null);
            showToast('Note deleted', 'warning');
        }
    });
}

/**
 * Select a note.
 */
function selectNote(id) {
    activeNoteId = id;
    const note = notes.find(n => n.id === id);
    renderNotesList();
    renderEditor(note);
}

/**
 * Create a new note.
 */
async function createNote() {
    const id = store.generateId();
    const newNote = await store.saveNote({
        id,
        title: '',
        content: ''
    });
    notes.unshift(newNote);
    selectNote(id);
    showToast('New note created', 'online');
    
    // Focus title input
    setTimeout(() => {
        document.getElementById('titleInput')?.focus();
    }, 100);
}

/**
 * Escape HTML to prevent XSS.
 */
function escapeHtml(str) {
    if (!str) return '';
    return str
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;')
        .replace(/"/g, '&quot;');
}

/**
 * Initialize the app.
 */
async function init() {
    try {
        // Initialize store
        const storageInfo = await store.initStore();
        updateStorageStatus(storageInfo);
        
        // Load notes
        notes = store.getAllNotes();
        renderNotesList();
        
        // Enable new note button
        newNoteBtn.disabled = false;
        newNoteBtn.addEventListener('click', createNote);
        
        // Search
        searchInput.addEventListener('input', (e) => {
            searchQuery = e.target.value;
            renderNotesList();
        });
        
        // Keyboard shortcuts
        document.addEventListener('keydown', (e) => {
            if ((e.ctrlKey || e.metaKey) && e.key === 'n') {
                e.preventDefault();
                createNote();
            }
        });
        
        showToast(`Loaded ${notes.length} notes`, 'online');
        
    } catch (error) {
        console.error('Failed to initialize:', error);
        notesList.innerHTML = `
            <div class="empty-state" style="padding: 2rem; color: var(--danger);">
                <p>Failed to initialize: ${error.message}</p>
            </div>
        `;
        storageDot.className = 'status-dot error';
        storageType.textContent = 'Error';
        syncStatus.textContent = error.message;
    }
}

// Start the app
init();
