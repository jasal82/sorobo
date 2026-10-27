// Copyright (C) 2026 Johannes Asal
// SPDX-License-Identifier: AGPL-3.0-or-later
// See LICENSE in the project root for the full license and warranty disclaimer.

import React from 'react';
import ReactDOM from 'react-dom/client';
import { MantineProvider, localStorageColorSchemeManager } from '@mantine/core';
import { DatesProvider } from '@mantine/dates';
import { Notifications } from '@mantine/notifications';
import { ModalsProvider } from '@mantine/modals';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { RouterProvider } from 'react-router';
import dayjs from 'dayjs';
import 'dayjs/locale/de';
import customParseFormat from 'dayjs/plugin/customParseFormat';

import '@fontsource-variable/inter';
import '@mantine/core/styles.css';
import '@mantine/dates/styles.css';
import '@mantine/notifications/styles.css';
import '@mantine/dropzone/styles.css';
import '@mantine/spotlight/styles.css';
import '@mantine/charts/styles.css';
import 'mantine-datatable/styles.css';
import './styles.css';

import { theme } from './theme';
import { router } from './router';
import { WindowChrome, installDesktopBridge } from './lib/desktop';
import { takeOverLegacyStorage } from './lib/legacyStorage';

takeOverLegacyStorage();
dayjs.extend(customParseFormat);
dayjs.locale('de');
installDesktopBridge();

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { staleTime: 15_000, refetchOnWindowFocus: false, retry: 1 },
  },
});

const colorSchemeManager = localStorageColorSchemeManager({ key: 'sorobo-color-scheme' });

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <MantineProvider theme={theme} defaultColorScheme="light" colorSchemeManager={colorSchemeManager}>
      <DatesProvider settings={{ locale: 'de', firstDayOfWeek: 1, weekendDays: [0, 6] }}>
        <QueryClientProvider client={queryClient}>
          <ModalsProvider labels={{ confirm: 'Bestätigen', cancel: 'Abbrechen' }}>
            <Notifications position="top-right" limit={4} />
            <RouterProvider router={router} />
            <WindowChrome />
          </ModalsProvider>
        </QueryClientProvider>
      </DatesProvider>
    </MantineProvider>
  </React.StrictMode>,
);
