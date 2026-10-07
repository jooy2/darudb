/** The screens' entry point: the styles, the providers toasts and dialogs need, and the app. */
import 'plass-ui/styles.css';
import './styles.css';

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { PlConfirmProvider } from 'plass-ui/confirm';
import { PlToastProvider } from 'plass-ui/toast';

import { App } from './App.tsx';

const root = document.getElementById('root');

if (root === null) {
  throw new Error('the page has no #root element');
}

createRoot(root).render(
  <StrictMode>
    <PlToastProvider>
      <PlConfirmProvider>
        <App />
      </PlConfirmProvider>
    </PlToastProvider>
  </StrictMode>
);
