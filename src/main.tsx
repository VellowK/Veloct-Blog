import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import App from './App';
import { NotificationHost } from './components/Notifications';
import './styles.css';
import './attachment-settings.css';
import './knowledge-links.css';

const container = document.getElementById('root');
if (!container) {
  // Failing loudly is the only useful behaviour here: a missing root means the HTML and the bundle
  // disagree, and rendering nothing would look like a slow start.
  throw new Error('#root is missing from index.html');
}

createRoot(container).render(
  <StrictMode>
    <App />
    <NotificationHost />
  </StrictMode>,
);
