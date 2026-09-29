import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import App from './App';
import './styles.css';

const container = document.getElementById('root');
if (!container) {
  // Failing loudly is the only useful behaviour here: a missing root means the HTML and the bundle
  // disagree, and rendering nothing would look like a slow start.
  throw new Error('#root is missing from index.html');
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
