// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import type { ReactNode } from 'react';
import { createBrowserRouter, Navigate } from 'react-router';
import { Center, Loader } from '@mantine/core';
import { AppLayout } from './components/AppLayout';
import { DashboardPage } from './pages/Dashboard';
import { ErrorPage } from './pages/ErrorPage';
import { NotFoundPage } from './pages/NotFound';

// Pages are loaded when first opened; the layout and the dashboard come with the main bundle.
const page =
  <M,>(load: () => Promise<M>, render: (m: M) => ReactNode) =>
  async () => ({ element: render(await load()) });

const documents = () => import('./pages/documents/DocumentsPage');
const editor = () => import('./pages/documents/DocumentEditor');
const receiptEditor = () => import('./pages/receipts/ReceiptEditor');
const settings = () => import('./pages/settings/SettingsPage');

const loading = (
  <Center h="60vh">
    <Loader />
  </Center>
);

export const router = createBrowserRouter([
  { path: '/willkommen', lazy: page(() => import('./pages/Onboarding'), (m) => <m.OnboardingPage />), errorElement: <ErrorPage />, hydrateFallbackElement: loading },
  {
    path: '/',
    element: <AppLayout />,
    errorElement: <ErrorPage />,
    hydrateFallbackElement: loading,
    children: [
      {
        // Errors of a page are shown inside the layout, so the navigation stays usable.
        errorElement: <ErrorPage />,
        children: [
          { index: true, element: <DashboardPage /> },
          { path: 'angebote', lazy: page(documents, (m) => <m.DocumentsPage docType="quote" />) },
          { path: 'auftraege', lazy: page(documents, (m) => <m.DocumentsPage docType="order" />) },
          { path: 'lieferscheine', lazy: page(documents, (m) => <m.DocumentsPage docType="delivery" />) },
          { path: 'rechnungen', lazy: page(documents, (m) => <m.DocumentsPage docType="invoice" />) },
          { path: 'gutschriften', lazy: page(documents, (m) => <m.DocumentsPage docType="credit_note" />) },
          { path: 'dokumente/neu', lazy: page(editor, (m) => <m.DocumentEditorPage />) },
          { path: 'dokumente/:id', lazy: page(() => import('./pages/documents/DocumentView'), (m) => <m.DocumentViewPage />) },
          { path: 'dokumente/:id/bearbeiten', lazy: page(editor, (m) => <m.DocumentEditorPage />) },
          { path: 'offene-posten', lazy: page(() => import('./pages/receivables/OpenItemsPage'), (m) => <m.OpenItemsPage />) },
          { path: 'belege', lazy: page(() => import('./pages/receipts/ReceiptsPage'), (m) => <m.ReceiptsPage />) },
          { path: 'belege/neu', lazy: page(receiptEditor, (m) => <m.ReceiptEditorPage />) },
          { path: 'belege/:id', lazy: page(receiptEditor, (m) => <m.ReceiptEditorPage />) },
          { path: 'kontakte', lazy: page(() => import('./pages/contacts/ContactsPage'), (m) => <m.ContactsPage />) },
          { path: 'kontakte/:id', lazy: page(() => import('./pages/contacts/ContactDetailPage'), (m) => <m.ContactDetailPage />) },
          { path: 'zeiterfassung', lazy: page(() => import('./pages/time/TimePage'), (m) => <m.TimePage />) },
          { path: 'artikel', lazy: page(() => import('./pages/articles/ArticlesPage'), (m) => <m.ArticlesPage />) },
          { path: 'bank', lazy: page(() => import('./pages/banking/BankPage'), (m) => <m.BankPage />) },
          { path: 'kasse', lazy: page(() => import('./pages/cashbook/CashbookPage'), (m) => <m.CashbookPage />) },
          { path: 'buchhaltung', element: <Navigate to="/buchhaltung/journal" replace /> },
          { path: 'buchhaltung/journal', lazy: page(() => import('./pages/accounting/JournalPage'), (m) => <m.JournalPage />) },
          { path: 'buchhaltung/konten', lazy: page(() => import('./pages/accounting/AccountsPage'), (m) => <m.AccountsPage />) },
          { path: 'buchhaltung/konten/:number', lazy: page(() => import('./pages/accounting/LedgerPage'), (m) => <m.LedgerPage />) },
          { path: 'buchhaltung/anlagen', lazy: page(() => import('./pages/accounting/AssetsPage'), (m) => <m.AssetsPage />) },
          { path: 'steuern/ustva', lazy: page(() => import('./pages/tax/UstvaPage'), (m) => <m.UstvaPage />) },
          { path: 'steuern/zm', lazy: page(() => import('./pages/tax/ZmPage'), (m) => <m.ZmPage />) },
          { path: 'berichte', lazy: page(() => import('./pages/reports/StatementsPage'), (m) => <m.StatementsPage />) },
          { path: 'auswertungen', lazy: page(() => import('./pages/reports/AnalyticsPage'), (m) => <m.AnalyticsPage />) },
          { path: 'exporte', lazy: page(() => import('./pages/reports/ExportsPage'), (m) => <m.ExportsPage />) },
          { path: 'einstellungen', lazy: page(settings, (m) => <m.SettingsPage />) },
          { path: 'einstellungen/:tab', lazy: page(settings, (m) => <m.SettingsPage />) },
          { path: 'einstellungen/layouts/:id', lazy: page(() => import('./pages/settings/LayoutDesigner'), (m) => <m.LayoutDesignerPage />) },
          { path: '*', element: <NotFoundPage /> },
        ],
      },
    ],
  },
]);
