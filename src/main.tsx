import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '@fontsource/inter/400.css';
import '@fontsource/inter/500.css';
import '@fontsource/inter/600.css';
import '@fontsource/inter/700.css';
import './styles.css';
import { App } from './App';

// End-to-end tests run the page in a plain browser with the Rust side mocked (src/mocks.ts).
if (import.meta.env.MODE === 'e2e') await import('./mocks');

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
