// Starts the popup. Everything else is in App.svelte.
import { mount } from 'svelte';
import './app.css';
import App from './App.svelte';

export default mount(App, { target: document.getElementById('app')! });
