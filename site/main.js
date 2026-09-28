import { mount } from 'svelte';
import { addMessages, init } from 'svelte-i18n';
import { website } from '../i18n/en.json';
import Site from './Site.svelte';
import './styles.css';

addMessages('en', { website });
init({ fallbackLocale: 'en', initialLocale: 'en' });
mount(Site, { target: document.getElementById('app') });
