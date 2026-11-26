import { mount } from 'svelte';
import 'uplot/dist/uPlot.min.css';
import './app.css';
import App from './App.svelte';

export default mount(App, { target: document.getElementById('app')! });
