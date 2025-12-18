/**
 * EntiDB Todo App
 * 
 * A practical todo application demonstrating EntiDB usage in the browser.
 */

import {
    initStore,
    createTodo,
    getAllTodos,
    toggleTodo,
    deleteTodo,
    clearCompleted,
    getCounts
} from './store.js';

// State
let currentFilter = 'all';

// DOM Elements
const loadingEl = document.getElementById('loading');
const appEl = document.getElementById('app');
const todoInputEl = document.getElementById('todoInput');
const addBtnEl = document.getElementById('addBtn');
const todoListEl = document.getElementById('todoList');
const totalCountEl = document.getElementById('totalCount');
const activeCountEl = document.getElementById('activeCount');
const completedCountEl = document.getElementById('completedCount');
const clearCompletedEl = document.getElementById('clearCompleted');
const filterBtns = document.querySelectorAll('.filter-btn');

// Initialize
async function init() {
    try {
        await initStore();
        loadingEl.style.display = 'none';
        appEl.style.display = 'block';
        renderTodos();
    } catch (error) {
        loadingEl.innerHTML = `<p style="color: var(--danger);">Failed to initialize: ${error.message}</p>`;
        console.error('Failed to initialize:', error);
    }
}

// Render the todo list
function renderTodos() {
    const todos = getAllTodos();
    const counts = getCounts();
    
    // Update stats
    totalCountEl.textContent = counts.total;
    activeCountEl.textContent = counts.active;
    completedCountEl.textContent = counts.completed;
    
    // Filter todos
    const filtered = todos.filter(todo => {
        switch (currentFilter) {
            case 'active': return !todo.completed;
            case 'completed': return todo.completed;
            default: return true;
        }
    });
    
    // Render list
    if (filtered.length === 0) {
        todoListEl.innerHTML = `
            <div class="empty-state">
                <div class="empty-state-icon">📋</div>
                <p>${getEmptyMessage()}</p>
            </div>
        `;
        return;
    }
    
    todoListEl.innerHTML = filtered.map(todo => `
        <li class="todo-item ${todo.completed ? 'completed' : ''}" data-id="${todo.id}">
            <div class="todo-checkbox ${todo.completed ? 'completed' : ''}" onclick="toggleTodoHandler('${todo.id}')"></div>
            <span class="todo-title">${escapeHtml(todo.title)}</span>
            <span class="todo-date">${formatDate(todo.createdAt)}</span>
            <button class="todo-delete" onclick="deleteTodoHandler('${todo.id}')" title="Delete">×</button>
        </li>
    `).join('');
}

function getEmptyMessage() {
    switch (currentFilter) {
        case 'active': return 'No active todos. Great job!';
        case 'completed': return 'No completed todos yet.';
        default: return 'No todos yet. Add one above!';
    }
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

function formatDate(timestamp) {
    const date = new Date(timestamp);
    const now = new Date();
    const diff = now - date;
    
    if (diff < 60000) return 'just now';
    if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`;
    if (diff < 86400000) return `${Math.floor(diff / 3600000)}h ago`;
    if (diff < 604800000) return `${Math.floor(diff / 86400000)}d ago`;
    
    return date.toLocaleDateString();
}

// Event handlers
function addTodo() {
    const title = todoInputEl.value.trim();
    if (!title) return;
    
    createTodo(title);
    todoInputEl.value = '';
    renderTodos();
}

window.toggleTodoHandler = function(id) {
    toggleTodo(id);
    renderTodos();
};

window.deleteTodoHandler = function(id) {
    deleteTodo(id);
    renderTodos();
};

function handleClearCompleted() {
    const count = clearCompleted();
    if (count > 0) {
        renderTodos();
    }
}

function setFilter(filter) {
    currentFilter = filter;
    filterBtns.forEach(btn => {
        btn.classList.toggle('active', btn.dataset.filter === filter);
    });
    renderTodos();
}

// Event listeners
addBtnEl.addEventListener('click', addTodo);

todoInputEl.addEventListener('keypress', (e) => {
    if (e.key === 'Enter') {
        addTodo();
    }
});

clearCompletedEl.addEventListener('click', handleClearCompleted);

filterBtns.forEach(btn => {
    btn.addEventListener('click', () => {
        setFilter(btn.dataset.filter);
    });
});

// Start the app
init();
